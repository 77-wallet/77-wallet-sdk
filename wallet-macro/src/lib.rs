#![allow(async_fn_in_trait)]
pub use macros;

pub trait QueryReqFieldsTrait {
    fn field_names() -> Vec<syn::Ident>;
}
