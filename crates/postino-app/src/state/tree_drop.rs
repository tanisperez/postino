//! Moving a request by dragging it onto another folder of the sidebar tree. Pure logic over
//! workspace ids, so it is unit-tested without `gpui`; `views/sidebar.rs` tracks the folder under
//! the pointer and `views/root.rs` moves the file. The workspace root is the folder `""`.

/// The folder holding `id`, `""` at the workspace root. `a/b/c.postino` gives `a/b`.
pub fn parent_folder(id: &str) -> &str {
    id.rfind('/').map_or("", |ix| &id[..ix])
}

/// The folder a request dropped on the row `row_id` goes to: the row itself for a folder, the
/// folder holding it for a request.
pub fn row_drop_folder(row_id: &str, is_request: bool) -> &str {
    if is_request {
        parent_folder(row_id)
    } else {
        row_id
    }
}

/// Where `request_id` goes when dropped with the pointer over `hovered` (`None` outside the
/// tree): `None` as well when it is the folder the request is already in, since nothing would
/// change.
pub fn drop_target(request_id: &str, hovered: Option<String>) -> Option<String> {
    hovered.filter(|folder| folder != parent_folder(request_id))
}

/// Whether the row `row_id` is drawn as part of the drop target `folder`: the folder's own row
/// and every row below it. The root (`""`) highlights the whole tree instead, never single rows.
pub fn in_drop_target(row_id: &str, folder: &str) -> bool {
    !folder.is_empty()
        && row_id
            .strip_prefix(folder)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_folder_of_nested_and_root_ids() {
        assert_eq!(parent_folder("a/b/c.postino"), "a/b");
        assert_eq!(parent_folder("a/b"), "a");
        assert_eq!(parent_folder("c.postino"), "");
    }

    #[test]
    fn a_folder_row_is_its_own_drop_folder_and_a_request_row_gives_its_parent() {
        assert_eq!(row_drop_folder("auth", false), "auth");
        assert_eq!(row_drop_folder("auth/login.postino", true), "auth");
        assert_eq!(row_drop_folder("ping.postino", true), "");
    }

    #[test]
    fn drop_target_skips_the_folder_the_request_is_already_in() {
        assert_eq!(drop_target("auth/login.postino", Some("auth".into())), None);
        assert_eq!(drop_target("ping.postino", Some(String::new())), None);
        assert_eq!(drop_target("auth/login.postino", None), None);
        assert_eq!(
            drop_target("auth/login.postino", Some("users".into())),
            Some("users".to_string())
        );
        assert_eq!(
            drop_target("auth/login.postino", Some(String::new())),
            Some(String::new())
        );
        assert_eq!(
            drop_target("ping.postino", Some("auth/admin".into())),
            Some("auth/admin".to_string())
        );
    }

    #[test]
    fn in_drop_target_covers_the_folder_row_and_everything_below_it() {
        assert!(in_drop_target("auth", "auth"));
        assert!(in_drop_target("auth/login.postino", "auth"));
        assert!(in_drop_target("auth/admin/reset.postino", "auth"));
        assert!(!in_drop_target("auth-extra", "auth"));
        assert!(!in_drop_target("users/list.postino", "auth"));
        assert!(!in_drop_target("auth", "auth/admin"));
    }

    #[test]
    fn the_root_target_highlights_no_single_row() {
        assert!(!in_drop_target("ping.postino", ""));
        assert!(!in_drop_target("auth", ""));
    }
}
