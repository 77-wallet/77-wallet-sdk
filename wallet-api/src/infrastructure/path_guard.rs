use std::path::{Component, Path};

use crate::error::service::ServiceError;

pub(crate) fn validate_path_segment<'a>(
    segment: &'a str,
    label: &str,
) -> Result<&'a str, ServiceError> {
    let path = Path::new(segment);
    let mut components = path.components();
    let Some(Component::Normal(_)) = components.next() else {
        return Err(ServiceError::Parameter(format!("invalid {label} path segment")));
    };
    if components.next().is_some() {
        return Err(ServiceError::Parameter(format!("invalid {label} path segment")));
    }
    if segment.as_bytes().contains(&0) {
        return Err(ServiceError::Parameter(format!("invalid {label} path segment")));
    }
    Ok(segment)
}

#[cfg(test)]
mod tests {
    use super::validate_path_segment;

    #[test]
    fn validate_path_segment_accepts_plain_value() {
        let segment =
            validate_path_segment("TXYZ1234567890", "wallet address").expect("plain segment");

        assert_eq!(segment, "TXYZ1234567890");
    }

    #[test]
    fn validate_path_segment_rejects_traversal() {
        let err = validate_path_segment("../secret", "wallet address")
            .expect_err("traversal must be rejected");

        assert!(err.to_string().contains("invalid wallet address path segment"));
    }

    #[test]
    fn validate_path_segment_rejects_nested_path() {
        let err = validate_path_segment("wallet/root", "wallet address")
            .expect_err("nested path must be rejected");

        assert!(err.to_string().contains("invalid wallet address path segment"));
    }
}
