//! Deterministic cross-crate migration ordering ([DAT-03]).

use std::collections::{HashMap, HashSet, VecDeque};

use crate::base::RiverbaseResult;

use super::store::DomainStoreSpec;

/// One embedded Diesel migration runner for a domain crate.
#[derive(Debug, Clone, Copy)]
pub struct MigrationSpec {
    /// Name.
    pub name: &'static str,
    /// Depends on.
    pub depends_on: &'static [&'static str],
    /// Run.
    pub run: fn(&str) -> RiverbaseResult<()>,
}

impl MigrationSpec {
    /// Build from store spec.
    pub fn from_store_spec(spec: &DomainStoreSpec) -> Option<Self> {
        let name = spec.migration_name?;
        let run = spec.migrate?;
        Some(Self {
            name,
            depends_on: spec.migration_after,
            run,
        })
    }
}

/// Collect migration specs from store specs, preserving first-seen order for ties.
pub fn migration_specs_from_store_specs<'a>(
    specs: impl IntoIterator<Item = &'a DomainStoreSpec>,
) -> Vec<MigrationSpec> {
    specs
        .into_iter()
        .filter_map(MigrationSpec::from_store_spec)
        .collect()
}

/// Topologically sort and run pending migrations once ([DAT-03]).
pub fn run_ordered_migrations(dsn: &str, migrations: &[MigrationSpec]) -> RiverbaseResult<()> {
    if migrations.is_empty() {
        return Ok(());
    }

    let names: HashSet<&str> = migrations.iter().map(|m| m.name).collect();
    for spec in migrations {
        for dep in spec.depends_on {
            if !names.contains(dep) {
                return Err(crate::errors::DOM_020
                    .with_data(format!("{} depends on missing {dep}", spec.name)));
            }
        }
    }

    let order = topo_sort(migrations)?;
    for spec in order {
        (spec.run)(dsn)?;
    }
    Ok(())
}

/// Run migrations declared on store specs in dependency order.
pub fn run_store_migrations(dsn: &str, specs: &[DomainStoreSpec]) -> RiverbaseResult<()> {
    let migrations = migration_specs_from_store_specs(specs);
    run_ordered_migrations(dsn, &migrations)
}

fn topo_sort(migrations: &[MigrationSpec]) -> RiverbaseResult<Vec<MigrationSpec>> {
    let mut by_name: HashMap<&str, MigrationSpec> = HashMap::new();
    for spec in migrations {
        if by_name.insert(spec.name, *spec).is_some() {
            return Err(crate::errors::DOM_021.with_data(spec.name.to_string()));
        }
    }

    let mut indegree: HashMap<&str, usize> = by_name.keys().map(|name| (*name, 0)).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for spec in migrations {
        for dep in spec.depends_on {
            *indegree.entry(spec.name).or_default() += 1;
            dependents.entry(dep).or_default().push(spec.name);
        }
    }

    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .into_iter()
        .collect();
    let mut queue_sorted: Vec<&str> = queue.iter().copied().collect();
    queue_sorted.sort_unstable();
    queue = queue_sorted.into();

    let mut ordered = Vec::with_capacity(migrations.len());
    while let Some(name) = queue.pop_front() {
        ordered.push(*by_name.get(name).expect("migration name exists in batch"));
        if let Some(children) = dependents.get(name) {
            let mut next = Vec::new();
            for child in children {
                let entry = indegree.get_mut(child).expect("child in batch");
                *entry = entry.saturating_sub(1);
                if *entry == 0 {
                    next.push(*child);
                }
            }
            next.sort_unstable();
            for child in next {
                queue.push_back(child);
            }
        }
    }

    if ordered.len() != migrations.len() {
        return Err(crate::errors::DOM_022.with_data("cycle in migration_after graph"));
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datastore::ResourceRegistry;

    fn noop(_dsn: &str) -> RiverbaseResult<()> {
        Ok(())
    }

    #[test]
    fn topo_sort_respects_dependencies() {
        let migrations = [
            MigrationSpec {
                name: "b",
                depends_on: &["a"],
                run: noop,
            },
            MigrationSpec {
                name: "a",
                depends_on: &[],
                run: noop,
            },
            MigrationSpec {
                name: "c",
                depends_on: &["b"],
                run: noop,
            },
        ];
        let order = topo_sort(&migrations)
            .expect("sort")
            .into_iter()
            .map(|m| m.name)
            .collect::<Vec<_>>();
        assert_eq!(order, vec!["a", "b", "c"]);
    }

    #[test]
    fn store_specs_without_migration_name_are_skipped() {
        let spec = DomainStoreSpec::new(ResourceRegistry::default());
        assert!(migration_specs_from_store_specs([&spec]).is_empty());
    }
}
