table! {
    entries (id) {
        id -> Int4,
        keyword_id -> Int4,
        idx -> Int4,
        creation_ts -> Timestamp,
        created_by -> Varchar,
    }
}

table! {
    keywords (id) {
        id -> Int4,
        name -> Varchar,
        chan -> Varchar,
    }
}
