mod macros;
extern crate proc_macro;

#[proc_macro_derive(Resource, attributes(resource))]
pub fn derive_entity(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    crate::macros::entity::expand_derive_entity(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

// // 第一个宏：QueryReqFields
// #[proc_macro_derive(QueryReqFields)]
// pub fn derive_query_req_fields(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
//     let input = syn::parse_macro_input!(input as syn::DeriveInput);

//     crate::macros::request::expand_derive_request(input)
//         .unwrap_or_else(syn::Error::into_compile_error)
//         .into()
// }
