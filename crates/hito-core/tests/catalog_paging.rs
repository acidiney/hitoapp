use hito_core::test_support::support;

use hito_core::Search;
use support::catalog;
use uuid::Uuid;

#[tokio::test]
async fn catalog_pages_preserve_case_insensitive_order_and_filtered_totals() {
    let (_dir, hito, actor, library) = catalog().await;
    let mut items = Vec::new();
    for offset in [0, 2, 4] {
        let page = hito
            .catalog
            .recordings(
                &actor,
                Search {
                    offset: Some(offset),
                    limit: Some(2),
                    ..Search::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(page.total, 5);
        assert_eq!(page.offset, offset);
        assert_eq!(page.limit, 2);
        items.extend(page.items);
    }
    assert_eq!(items.len(), 5);
    let keys: Vec<_> = items
        .iter()
        .map(|item| (item.title.to_ascii_lowercase(), item.id.clone()))
        .collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted);
    let unique: std::collections::HashSet<_> = items.iter().map(|item| &item.id).collect();
    assert_eq!(unique.len(), 5);

    let filtered = hito
        .catalog
        .recordings(
            &actor,
            Search {
                q: Some("al".into()),
                library: Some(library),
                offset: Some(1),
                limit: Some(1),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(filtered.total, 2);
    assert_eq!(filtered.items[0].id, items[1].id);
    let hidden = hito
        .catalog
        .recordings(
            &actor,
            Search {
                library: Some(Uuid::new_v4().to_string()),
                ..Search::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(hidden.total, 0);
    assert!(hidden.items.is_empty());
}
