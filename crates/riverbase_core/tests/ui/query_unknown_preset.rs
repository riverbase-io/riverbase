riverbase_core::query_resource! {
    Broken name "broken" {
        meta {
            title: "Broken",
            default_order: [id.desc],
            scope: none,
        }
        fields {
            field id { preset: Uuid, label: "ID", identifier }
            field title { preset: Strng, label: "Title" }
        }
        binding source "broken" {}
    }
}

fn main() {}
