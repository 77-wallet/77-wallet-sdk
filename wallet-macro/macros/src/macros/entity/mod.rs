pub(crate) mod attributes;
use attributes::derive_attr::Resource;
use proc_macro::TokenStream;
use quote::quote;
use syn::Fields;

pub(crate) fn expand_derive_entity(
    input: syn::DeriveInput,
) -> syn::Result<proc_macro2::TokenStream> {
    DeriveResource::new(input)
}

#[derive(Debug)]
// #[allow(dead_code)]
struct DeriveResource {
    // struct_ident: syn::Ident,
    // struct_generics: syn::Generics,
    // schema_name: Option<syn::Ident>,
    // sqlite_table_name: String,
    // primary_keys: Vec<(syn::Ident, syn::Ident)>,
    // constraint: String,
    // fields: Vec<Field>,
    // error: syn::Ident,
}

#[derive(Debug, Clone)]
struct Field {
    ident: syn::Ident,
    detail: Option<String>,
}
impl DeriveResource {
    fn new(input: syn::DeriveInput) -> Result<proc_macro2::TokenStream, syn::Error> {
        let name = &input.ident;
        let Resource {
            // schema_name,
            query_req,
            // upsert_req,
            sqlite_table_name,
            // constraint,
            // primary_key,
            // table_iden: _,
            // error,
        } = Resource::from_attributes(&input.attrs)?;

        let query_req_type = if let Some(syn::Lit::Str(lit_str)) = &query_req {
            lit_str.parse::<syn::Type>()?
        } else {
            return Err(syn::Error::new_spanned(
                query_req,
                "query_req must be a valid type path",
            ));
        };

        let fields = match input.data {
            syn::Data::Struct(ref data_struct) => match data_struct.fields {
                Fields::Named(ref fields_named) => fields_named
                    .named
                    .iter()
                    .filter_map(|field| {
                        let ident = field.ident.as_ref()?;
                        let field_attr =
                            attributes::field_attr::Resource::try_from_attributes(&field.attrs)
                                .ok()?;

                        let detail = match field_attr {
                            Some(attr) => attr
                                .detail
                                .and_then(|d| parse_lit_string(&d).ok())
                                .map(|d| d.to_string()),
                            None => None,
                        };
                        Some(Field {
                            ident: ident.clone(),
                            detail,
                        })
                    })
                    .collect::<Vec<_>>(),
                _ => panic!("Only named fields are supported"),
            },
            _ => panic!("DeriveEntity can only be used with structs"),
        };

        let mut conditions = Vec::new();
        let mut bindings = Vec::new();

        for field in fields {
            let ident = field.ident;
            let field_name = ident.to_string();
            let detail = field.detail;
            if let Some(_detail) = detail {
                conditions.push(quote! {
                    if let Some(ref #ident) = query_req.#ident {
                        conditions.push(format!("{} = ?", #field_name));
                    }
                });
                bindings.push(quote! {
                    if let Some(ref #ident) = query_req.#ident {
                        query = query.bind(#ident);
                    }
                });
            }
        }

        let expanded = quote! {
            impl #name {
                pub async fn detail<'a, E>(
                    exec: E,
                    query_req: &#query_req_type,
                ) -> Result<Option<Self>, crate::Error>
                where
                    E: sqlx::Executor<'a, Database = sqlx::Sqlite>,
                {
                    let mut sql = format!("SELECT * FROM {}", #sqlite_table_name);
                    let mut conditions = Vec::new();

                    #(#conditions)*

                    if !conditions.is_empty() {
                        sql.push_str(" WHERE ");
                        sql.push_str(&conditions.join(" AND "));
                    }
                    let mut query = sqlx::query_as::<_, #name>(&sql);

                    #(#bindings)*

                    query
                        .fetch_optional(exec)
                        .await
                        .map_err(|e| crate::Error::Database(e.into()))
                }

            }


        };

        Ok(TokenStream::from(expanded).into())
    }
}

fn parse_lit_string(lit: &syn::Lit) -> syn::Result<TokenStream> {
    match lit {
        syn::Lit::Str(lit_str) => lit_str
            .value()
            .parse()
            .map_err(|_| syn::Error::new_spanned(lit, "attribute not valid")),
        _ => Err(syn::Error::new_spanned(lit, "attribute must be a string")),
    }
}
