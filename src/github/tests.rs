use super::*;

fn dotfiles() -> GitHubRepository {
    GitHubRepository {
        owner: crate::configuration::RepositoryOwner::from("Alice"),
        repository: crate::configuration::RepositoryName::from("dotfiles"),
    }
}

fn item_reported_as(kind: &str, encoded: Option<&str>) -> Content {
    let mut document = serde_json::json!({
        "name": "configuration.json",
        "path": "configurations/configuration.json",
        "sha": "8d3dfe1",
        "size": 7,
        "url": "https://api.github.invalid/contents",
        "type": kind,
        "_links": { "self": "https://api.github.invalid/contents" }
    });
    if let Some(encoded) = encoded {
        document["content"] = serde_json::Value::String(encoded.to_owned());
    }

    serde_json::from_value(document).expect("a content item the API could have sent")
}

#[test]
fn a_file_whose_content_the_response_carries_is_read_as_that_content() {
    let item = item_reported_as("file", Some("aGVsbG8="));

    assert_eq!(decoded_content_of(&item, &dotfiles()).unwrap(), "hello");
}

#[test]
fn a_file_whose_content_the_response_omits_is_refused_by_name_rather_than_dropped() {
    let item = item_reported_as("dir", None);

    let error = decoded_content_of(&item, &dotfiles()).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("configurations/configuration.json"),
        "expected the message to name the file it could not read, got: {error}"
    );
}
