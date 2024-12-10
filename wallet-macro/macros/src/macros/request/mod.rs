pub(crate) mod attributes;

// pub(crate) fn expand_derive_request(
//     input: syn::DeriveInput,
// ) -> syn::Result<proc_macro2::TokenStream> {
//     let struct_name = &input.ident;
//     let fields = match input.data {
//         Data::Struct(data_struct) => match data_struct.fields {
//             Fields::Named(ref named_fields) => named_fields
//                 .named
//                 .iter()
//                 .map(|f| {
//                     let ident = f.ident.as_ref().unwrap().to_string();
//                     quote! { #ident }
//                 })
//                 .collect::<Vec<_>>(),
//             _ => panic!("QueryReqFields can only be derived for structs with named fields"),
//         },
//         _ => panic!("QueryReqFields can only be derived for structs"),
//     };

//     let gen = quote! {
//         use wallet_macro::QueryReqFieldsTrait;
//         use syn::Ident;

//         #[automatically_derived]
//         impl QueryReqFieldsTrait for #struct_name {
//             fn field_names() -> Vec<Ident> {
//                 vec![#(#fields),*]
//             }
//         }
//     };

//     Ok(gen.into())
// }
