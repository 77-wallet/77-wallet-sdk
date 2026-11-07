pub mod address;
pub mod chain;
pub mod coin;
pub mod strategy;
pub mod wallet;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pages<T> {
    pub total_elements: i64,
    pub total_pages: i64,
    pub first: bool,
    pub last: bool,
    pub size: i64,
    pub number: i32,
    pub sort: Sort,
    pub pageable: Pageable,
    pub number_of_elements: i64,
    pub empty: bool,
    pub content: Vec<T>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub empty: bool,
    pub unsorted: bool,
    pub sorted: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pageable {
    pub offset: i64,
    pub sort: Sort,
    pub page_number: i64,
    pub page_size: i64,
    pub unpaged: bool,
    pub paged: bool,
}
