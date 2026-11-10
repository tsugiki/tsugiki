use super::*;

fn latest_release() -> RestPath {
    RestPath::parse("/repos/tsugiki/tsugiki/releases/latest").unwrap()
}

#[test]
fn a_public_request_carries_no_authorization_header() {
    let request = Https::new().public_request(&latest_release()).unwrap();
    let headers = request.headers_ref().unwrap();
    assert!(headers.get("authorization").is_none());
    assert_eq!(headers["accept"], "application/vnd.github+json");
}

#[test]
fn a_public_request_goes_to_the_path_on_api_github_com() {
    let request = Https::new().public_request(&latest_release()).unwrap();
    assert_eq!(
        request.uri_ref().unwrap(),
        "https://api.github.com/repos/tsugiki/tsugiki/releases/latest"
    );
}
