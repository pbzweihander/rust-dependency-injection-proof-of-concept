use derive_syn_parse::Parse;
use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_error::{abort, abort_call_site, proc_macro_error};
use quote::quote;
use syn::ext::IdentExt;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, token, DeriveInput, Ident, Path, Token, Type};

mod kw {
    syn::custom_keyword!(fallible);
    syn::custom_keyword!(arc);
    syn::custom_keyword!(wrap);
    syn::custom_keyword!(with);
    syn::custom_keyword!(depend);
    syn::custom_keyword!(error);
}

macro_rules! unwrap_syn_result {
    ($r:expr) => {
        match $r {
            Ok(o) => o,
            Err(e) => {
                return ::proc_macro::TokenStream::from(e.to_compile_error());
            }
        }
    };
}

#[derive(Parse)]
struct AdditionalOptions<O: Parse> {
    #[prefix(Token![,])]
    #[call(Punctuated::parse_terminated)]
    options: Punctuated<O, Token![,]>,
}

impl<O: Parse> Default for AdditionalOptions<O> {
    fn default() -> Self {
        Self {
            options: Default::default(),
        }
    }
}

#[derive(Parse)]
struct ProvideAttr {
    ty: ProvideType,
    #[peek(token::Comma)]
    options: Option<AdditionalOptions<ProvideOption>>,
}

#[derive(Parse)]
enum ProvideType {
    #[peek(token::SelfValue, name = "self")]
    WithSelf(Token![self]),
    #[peek(Ident::peek_any, name = "item")]
    WithType(Box<Type>),
}

#[derive(Parse)]
enum ProvideOption {
    #[peek(token::Box, name = "box")]
    Box(Token![box]),
    #[peek(kw::arc, name = "arc")]
    Arc(kw::arc),
    #[peek(kw::fallible, name = "fallible")]
    Fallible {
        #[prefix(kw::fallible)]
        #[paren]
        _paren: token::Paren,
        #[prefix(kw::error in _paren)]
        #[prefix(Token![=] in _paren)]
        #[inside(_paren)]
        error_type: Box<Type>,
    },
    #[peek(token::Async, name = "async")]
    Async(Token![async]),
    #[peek(kw::wrap, name = "wrap")]
    Wrap {
        #[prefix(kw::wrap)]
        #[paren]
        _paren: token::Paren,
        #[inside(_paren)]
        wrap_type: Box<Type>,
        #[prefix(kw::with)]
        wrap_with: Path,
    },
}

impl ProvideOption {
    fn wrap_type(&self, ty: &TokenStream2) -> TokenStream2 {
        match self {
            Self::Box(_) => quote! { ::std::boxed::Box<#ty> },
            Self::Arc(_) => quote! { ::std::sync::Arc<#ty> },
            Self::Async(_) => quote! { ::dipoc::boxed::BoxFuture<'__module, #ty> },
            Self::Fallible { error_type, .. } => quote! { ::std::result::Result<#ty, #error_type> },
            Self::Wrap { wrap_type, .. } => quote! { #wrap_type<#ty> },
        }
    }

    fn wrap_expr(&self, expr: &TokenStream2) -> TokenStream2 {
        match self {
            Self::Box(_) => {
                quote! { ::std::boxed::Box::new(#expr) }
            }
            Self::Arc(_) => quote! { ::std::sync::Arc::new(#expr) },
            Self::Async(_) => quote! { ::std::boxed::Box::pin(async move { #expr }) },
            Self::Fallible { .. } => quote! { Ok(#expr) },
            Self::Wrap { wrap_with, .. } => quote! { #wrap_with(#expr) },
        }
    }
}

#[derive(Parse)]
enum DependOption {
    #[peek(token::Try, name = "try")]
    Try {
        #[prefix(token::Try)]
        #[paren]
        _paren: token::Paren,
        #[prefix(kw::error in _paren)]
        #[prefix(Token![=] in _paren)]
        #[inside(_paren)]
        error_type: Box<Type>,
    },
    #[peek(token::Await, name = "await")]
    Await(Token![await]),
    #[peek(token::Default, name = "default")]
    Default(Token![default]),
    #[peek(kw::wrap, name = "wrap")]
    Wrap {
        #[prefix(kw::wrap)]
        #[paren]
        _paren: token::Paren,
        #[inside(_paren)]
        wrap_type: Box<Type>,
        #[prefix(kw::with)]
        wrap_with: Path,
    },
}

impl DependOption {
    fn wrap_type(&self, ty: &TokenStream2) -> Option<TokenStream2> {
        match self {
            Self::Try { error_type, .. } => {
                Some(quote! { ::std::result::Result<#ty, #error_type> })
            }
            Self::Await(_) => Some(quote! { ::dipoc::boxed::BoxFuture<'__module, #ty> }),
            Self::Default(_) => None,
            Self::Wrap { wrap_type, .. } => Some(quote! { #wrap_type<#ty> }),
        }
    }

    fn wrap_expr(&self, expr: &TokenStream2) -> TokenStream2 {
        match self {
            Self::Try { .. } => quote! { #expr? },
            Self::Await(_) => quote! { #expr.await },
            Self::Default(_) => quote! { Default::default() },
            Self::Wrap { wrap_with, .. } => quote! { #wrap_with(#expr) },
        }
    }

    fn sync_needed(&self) -> bool {
        matches!(self, Self::Await(_))
    }
}

#[proc_macro_error]
#[proc_macro_derive(Provider, attributes(provide, depend))]
pub fn derive_provider(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);

    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let impl_generics = &input.generics.params;
    let ident = &input.ident;

    let item_struct = if let syn::Data::Struct(s) = &input.data {
        s
    } else {
        abort!(input, "Only structs are currently supported")
    };

    let attr = input
        .attrs
        .iter()
        .find(|a| a.path.is_ident("provide"))
        .unwrap_or_else(|| abort_call_site!("Attribute `provide` not found"));
    let provide_attr = unwrap_syn_result!(attr.parse_args::<ProvideAttr>());

    let fields = item_struct
        .fields
        .iter()
        .map(|f| {
            let options = f
                .attrs
                .iter()
                .filter(|a| a.path.is_ident("depend"))
                .map(|a| a.parse_args_with(Punctuated::<DependOption, Token![,]>::parse_terminated))
                .collect::<syn::Result<Vec<_>>>()
                .map(|options| {
                    options
                        .into_iter()
                        .flat_map(IntoIterator::into_iter)
                        .collect::<Vec<_>>()
                });
            let ident = f
                .ident
                .as_ref()
                .unwrap_or_else(|| abort!(f, "Field must be named"));
            let ty = &f.ty;
            options.map(|options| (options, ident, ty))
        })
        .collect::<syn::Result<Vec<_>>>();
    let fields = unwrap_syn_result!(fields);

    let interface_type = match provide_attr.ty {
        ProvideType::WithType(ty) => {
            quote! { #ty }
        }
        ProvideType::WithSelf(_) => {
            quote! { #ident }
        }
    };

    let mut sync_needed = false;
    let (mut dependencies, depend_exprs, depend_idents) = fields
        .iter()
        .map(|(options, ident, ty)| {
            let dep_ty = options
                .iter()
                .try_rfold(quote! { #ty }, |ty, option| option.wrap_type(&ty));
            let dep_ty = dep_ty.map(|ty| quote! { ::dipoc::HasProvider<'__module, #ty> });
            let dep_expr = options.iter().fold(
                quote! { <__Module as #dep_ty>::provide(__module) },
                |expr, option| option.wrap_expr(&expr),
            );
            sync_needed |= options.iter().any(DependOption::sync_needed);
            (dep_ty, quote! { let #ident: #ty = #dep_expr; }, ident)
        })
        .fold(
            (vec![], vec![], vec![]),
            |(mut v1, mut v2, mut v3), (i1, i2, i3)| {
                let _ = i1.map(|i1| v1.push(i1));
                v2.push(i2);
                v3.push(i3);
                (v1, v2, v3)
            },
        );
    if sync_needed {
        dependencies.push(quote! { Sync });
    }

    let provided_expr = quote! {{
        use ::dipoc::HasProvider;
        #(#depend_exprs)
        *
        Self {
            #(#depend_idents),
            *
        }
    }};

    let (interface_type, wrap_expr) = provide_attr
        .options
        .unwrap_or_default()
        .options
        .iter()
        .fold(
            (quote! { #interface_type }, provided_expr),
            |(ty, expr), o| {
                let ty = o.wrap_type(&ty);
                let expr = o.wrap_expr(&expr);
                let expr = quote! { #expr as #ty };
                (ty, expr)
            },
        );

    let res = quote! {
        impl<
            '__module,
            __Module: #(#dependencies)+*,
            #impl_generics
        > ::dipoc::Provider<'__module, __Module> for #ident #ty_generics #where_clause {
            type Interface = #interface_type;

            fn provide(__module: &'__module __Module) -> Self::Interface {
                #wrap_expr
            }
        }
    };
    res.into()
}
