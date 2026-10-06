use super::*;
use crate::{EntityMetadata, EntityPlacementRef};

fn symbol(name: &str) -> EntityRef {
    EntityRef::Symbol(name.into())
}

fn delete(name: &str) -> Command {
    Command::Delete {
        entity: symbol(name),
    }
}

fn assign(entity: EntityRef, name: &str) -> Command {
    Command::SetMetadata {
        entity,
        metadata: EntityMetadata {
            symbolic_id: Some(name.into()),
            classes: Vec::new(),
        },
    }
}

fn count(commands: impl IntoIterator<Item = Command>) -> usize {
    let mut reports = BatchSymbolReports::default();
    for mut command in commands {
        reports.add(&mut command);
    }
    reports.reports()
}

#[test]
fn each_distinct_symbol_reports_once_however_often_it_is_named() {
    assert_eq!(count([delete("a"), delete("a"), delete("b")]), 2);
    assert_eq!(
        count([Command::PlaceEntity {
            entity: symbol("a"),
            placement: EntityPlacementRef {
                parent: Some(symbol("b")),
                before: Some(symbol("c")),
            },
        }]),
        3
    );
}

#[test]
fn a_symbol_reports_once_more_for_each_command_that_can_move_it() {
    // An assignment without a reference reports nothing.
    assert_eq!(count([assign(EntityRef::Alias(1), "a")]), 0);
    // A moved symbol can resolve to one more handle per reference, in any
    // order of its references and assignments.
    assert_eq!(
        count([delete("a"), assign(EntityRef::Alias(1), "a"), delete("a")]),
        2
    );
    assert_eq!(
        count([assign(EntityRef::Alias(1), "a"), delete("a"), delete("a")]),
        2
    );
    // Never more reports than references.
    assert_eq!(
        count([
            assign(EntityRef::Alias(1), "a"),
            assign(EntityRef::Alias(2), "a"),
            delete("a"),
        ]),
        1
    );
    // A creation carrying the symbol counts as an assignment.
    assert_eq!(
        count([
            delete("a"),
            Command::Create {
                alias: 1,
                metadata: EntityMetadata {
                    symbolic_id: Some("a".into()),
                    classes: Vec::new(),
                },
                adopt: false,
            },
            delete("a"),
        ]),
        2
    );
}

#[test]
fn text_bytes_follow_each_counted_report() {
    let mut reports = BatchSymbolReports::default();
    for mut command in [
        delete("abc"),
        delete("abc"),
        delete("de"),
        assign(EntityRef::Alias(1), "de"),
        delete("de"),
    ] {
        reports.add(&mut command);
    }
    assert_eq!(reports.reports(), 3);
    assert_eq!(reports.text_bytes(), 3 + 2 * 2);
}
