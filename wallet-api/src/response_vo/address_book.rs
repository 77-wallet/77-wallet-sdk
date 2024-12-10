use wallet_database::entities::address_book::AddressBookEntity;

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddressBookResp {
    pub address_book: Option<AddressBookEntity>,
    pub first_transfer: bool,
}
