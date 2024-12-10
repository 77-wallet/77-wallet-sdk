pub mod derive_attr {
    use bae::FromAttributes;

    #[derive(Debug, FromAttributes)]
    pub struct Resource {
        // pub schema_name: Option<syn::Lit>,
        pub query_req: Option<syn::Lit>,
        // pub upsert_req: Option<syn::Lit>,
        pub sqlite_table_name: syn::Lit,
        // pub constraint: syn::Lit,
        // pub primary_key: syn::Lit,
        // pub table_iden: Option<()>,
        // pub error: Option<syn::Lit>,
    }
}

pub mod field_attr {
    use bae::FromAttributes;

    #[derive(Debug, Default, FromAttributes)]
    pub struct Resource {
        pub detail: Option<syn::Lit>,
        // pub fields: Option<syn::Lit>,
    }
}
