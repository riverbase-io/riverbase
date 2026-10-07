use riverbase_core::datastore::ResourceKey;

struct ProbeKey;
impl ResourceKey for ProbeKey {
    const NAME: &'static str = "probe";
    const SOURCE: &'static str = "probe";
    const SORT_COLUMNS: &'static [&'static str] = &["title"];
}

const _: () = riverbase_core::query::assert_sort_columns_covered(
    "ProbeEntity",
    "ProbeKey",
    ProbeKey::SORT_COLUMNS,
    &["_id"],
);

fn main() {}
