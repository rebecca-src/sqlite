use std::collections::HashMap;

use sqlite::{Type, Value};

mod common;

use common::{setup_english, setup_users};

macro_rules! ok(($result:expr) => ($result.unwrap()));

#[test]
fn bind_iter() {
    let connection = ok!(sqlite::open(":memory:"));
    ok!(connection.execute("CREATE TABLE users (id INTEGER, name STRING)"));
    let mut statement = ok!(connection.prepare("INSERT INTO users VALUES (:id, :name)"));

    let mut map = HashMap::<_, Value>::new();
    map.insert(":name", "Bob".to_string().into());
    map.insert(":id", 42.into());

    let mut cursor = ok!(statement.iter().bind_iter(map));
    assert!(cursor.next().is_none());
}

#[test]
fn iter() {
    let connection = setup_users(":memory:");
    ok!(connection.execute("INSERT INTO users VALUES (2, 'Bob', NULL, NULL, NULL)"));
    let query = "SELECT id, age FROM users ORDER BY 1 DESC";
    let mut statement = ok!(connection.prepare(query));

    let mut count = 0;
    for row in statement.iter().map(|row| ok!(row)) {
        let id = row.read::<i64, _>("id");
        if id == 1 {
            assert_eq!(row.read::<f64, _>("age"), 42.69);
        } else if id == 2 {
            assert_eq!(row.read::<Option<f64>, _>("age"), None);
        } else {
            assert!(false);
        }
        count += 1;
    }
    assert_eq!(count, 2);
}

#[test]
fn iter_column_count() {
    let connection = setup_english(":memory:");
    let query = "SELECT value FROM english WHERE value LIKE '%type'";
    let mut statement = ok!(connection.prepare(query));

    let cursor = statement.iter();
    assert_eq!(cursor.column_count(), 1);
}

#[test]
fn iter_column_type() {
    let connection = setup_english(":memory:");
    let query = "SELECT value FROM english WHERE value LIKE '%type'";
    let mut statement = ok!(connection.prepare(query));

    let cursor = statement.iter();
    assert_eq!(ok!(cursor.column_type(0)), Type::Null);
    assert_eq!(ok!(cursor.column_type("value")), Type::Null);

    ok!(statement.reset());
    let mut cursor = statement.iter();
    ok!(cursor.try_next());
    assert_eq!(ok!(cursor.column_type(0)), Type::String);
    assert_eq!(ok!(cursor.column_type("value")), Type::String);

    ok!(statement.reset());
    let mut count = 0;
    let mut cursor = statement.iter();
    while let Ok(Some(_)) = cursor.try_next() {
        assert_eq!(ok!(cursor.column_type(0)), Type::String);
        assert_eq!(ok!(cursor.column_type("value")), Type::String);
        count += 1;
    }
    assert_eq!(count, 6);
}

#[test]
fn iter_count() {
    let connection = setup_english(":memory:");
    let query = "SELECT value FROM english WHERE value LIKE '%type'";
    let mut statement = ok!(connection.prepare(query));

    assert_eq!(statement.iter().filter(|row| row.is_ok()).count(), 6);
}

#[test]
fn iter_with_exception() {
    let connection = ok!(sqlite::open(":memory:"));
    ok!(connection.execute("CREATE TABLE foo(x)"));
    ok!(connection
        .execute("CREATE TRIGGER bar BEFORE INSERT ON foo BEGIN SELECT RAISE(FAIL, 'buz'); END"));
    let mut statement = ok!(connection.prepare("INSERT INTO foo VALUES (0) RETURNING rowid;"));
    let results = statement.iter().collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    assert!(matches!(results[0], Err(_)));
}

#[test]
fn next_index() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));

    assert_eq!(row[0], Value::Integer(1));
    assert_eq!(row[2], Value::Float(42.69));

    assert_eq!(row["id"], Value::Integer(1));
    assert_eq!(row["age"], Value::Float(42.69));
}

#[test]
fn next_read_with_name() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert_eq!(row.read::<i64, _>("id"), 1);
    assert_eq!(row.read::<&str, _>("name"), "Alice");
    assert_eq!(row.read::<f64, _>("age"), 42.69);
    assert_eq!(row.read::<&[u8], _>("photo"), &[0x42u8, 0x69u8][..]);
}

#[test]
fn next_read_with_name_and_option() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert!(row.read::<Option<i64>, _>("id").is_some());
    assert!(row.read::<Option<&str>, _>("name").is_some());
    assert!(row.read::<Option<f64>, _>("age").is_some());
    assert!(row.read::<Option<&[u8]>, _>("photo").is_some());
    assert!(row.read::<Option<&str>, _>("email").is_none());
}

#[test]
fn next_take() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let mut row = ok!(ok!(statement.iter().next()));
    assert_eq!(row.take("name"), Value::String("Alice".into()));
    assert_eq!(row.take("name"), Value::Null);
}

#[test]
fn next_try_read_with_index() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert!(row.try_read::<f64, _>(0).is_err());
    assert!(row.try_read::<i64, _>(0).is_ok());
    assert!(row.try_read::<&str, _>(1).is_ok());
    assert!(row.try_read::<f64, _>(2).is_ok());
    assert!(row.try_read::<&[u8], _>(3).is_ok());
    assert!(row.try_read::<&str, _>(4).is_err());
}

#[test]
fn next_try_read_with_index_out_of_range() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert!(row.try_read::<&str, _>(5).is_err());
}

#[test]
fn next_try_read_with_index_and_option() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert!(row.try_read::<Option<f64>, _>(0).is_err());
    assert!(ok!(row.try_read::<Option<i64>, _>(0)).is_some());
    assert!(ok!(row.try_read::<Option<&str>, _>(1)).is_some());
    assert!(ok!(row.try_read::<Option<f64>, _>(2)).is_some());
    assert!(ok!(row.try_read::<Option<&[u8]>, _>(3)).is_some());
    assert!(ok!(row.try_read::<Option<&str>, _>(4)).is_none());
}

#[test]
fn next_try_read_with_name() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert!(row.try_read::<f64, _>("id").is_err());
    assert!(row.try_read::<i64, _>("id").is_ok());
    assert!(row.try_read::<&str, _>("name").is_ok());
    assert!(row.try_read::<f64, _>("age").is_ok());
    assert!(row.try_read::<&[u8], _>("photo").is_ok());
    assert!(row.try_read::<&str, _>("email").is_err());
}

#[test]
fn next_try_read_with_name_and_option() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let row = ok!(ok!(statement.iter().next()));
    assert!(row.try_read::<Option<f64>, _>("id").is_err());
    assert!(ok!(row.try_read::<Option<i64>, _>("id")).is_some());
    assert!(ok!(row.try_read::<Option<&str>, _>("name")).is_some());
    assert!(ok!(row.try_read::<Option<f64>, _>("age")).is_some());
    assert!(ok!(row.try_read::<Option<&[u8]>, _>("photo")).is_some());
    assert!(ok!(row.try_read::<Option<&str>, _>("email")).is_none());
}

#[test]
fn try_next_try_into() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));

    let mut cursor = statement.iter();
    let row = ok!(ok!(cursor.try_next()));
    assert!((&row[0]).try_into::<f64>().is_err());
    assert!((&row[0]).try_into::<i64>().is_ok());
    assert!((&row[1]).try_into::<&str>().is_ok());
    assert!((&row[2]).try_into::<f64>().is_ok());
    assert!((&row[3]).try_into::<&[u8]>().is_ok());
    assert!((&row[4]).try_into::<&str>().is_err());
}

#[test]
fn row_can_be_iterated() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));
    let mut cursor = statement.iter();
    let row = ok!(ok!(cursor.next()));
    let _ = row.iter();
}

#[test]
fn iterating_row_yields_all_items() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));
    let mut cursor = statement.iter();
    let row = ok!(ok!(cursor.next()));
    let row = row.iter();
    assert_eq!(5, row.count());
}

#[test]
fn iterating_row_yields_all_items_in_correct_order() {
    let connection = setup_users(":memory:");
    let query = "SELECT * FROM users";
    let mut statement = ok!(connection.prepare(query));
    let mut cursor = statement.iter();
    let row = ok!(ok!(cursor.next()));
    let mut row = row.iter();
    assert_eq!(Some(("id", &Value::Integer(1))), row.next());
    assert_eq!(
        Some(("name", &Value::String("Alice".to_owned()))),
        row.next()
    );
    assert_eq!(Some(("age", &Value::Float(42.69))), row.next());
    assert_eq!(Some(("photo", &Value::Binary(vec![66, 105]))), row.next());
    assert_eq!(Some(("email", &Value::Null)), row.next());
    assert_eq!(None, row.next());
}

#[test]
fn workflow() {
    let connection = setup_users(":memory:");

    let select = "SELECT id, name FROM users WHERE id = ?";
    let mut select = ok!(connection.prepare(select));
    let mut select = select.iter();

    let insert = "INSERT INTO users (id, name) VALUES (?, ?)";
    let mut insert = ok!(connection.prepare(insert));
    let insert = insert.iter();

    for _ in 0..10 {
        select = ok!(select.bind((1, 1)));
        let row = ok!(ok!(select.next()));
        assert_eq!(row.read::<i64, _>("id"), 1);
        assert_eq!(row.read::<&str, _>("name"), "Alice");
        assert!(select.next().is_none());
    }

    let mut select = ok!(select.bind((1, 42)));
    assert!(select.next().is_none());

    let mut insert = ok!(insert.bind::<&[Value]>(&[42.into(), String::from("Bob").into()][..]));
    assert!(insert.next().is_none());

    let mut select = ok!(select.bind((1, 42)));
    let row = ok!(ok!(select.next()));
    assert_eq!(row.read::<i64, _>("id"), 42);
    assert_eq!(row.read::<&str, _>("name"), "Bob");
    assert!(select.next().is_none());
}
