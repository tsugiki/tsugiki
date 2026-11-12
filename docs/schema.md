# The database schema

<!-- Written by `just schema` from crates/core/src/store/migrate.rs. Don't edit it by hand. -->

The tables of `tsugiki.db` as its migrations leave them, cache apart from state
([ADR 0010](adr/0010-one-sqlite-database-and-per-repository-mirrors.md)). `PK` marks a table's primary key
and `FK` a column that points at another table's row, and a column that may hold null says so. A line joins
the table that points to the one it points at, and its label names the columns.

## The cache: what GitHub says

### PRs

```mermaid
erDiagram
    pull_requests {
        TEXT repository PK
        INTEGER number PK
        TEXT title
        TEXT state
        INTEGER is_draft
        TEXT author "nullable"
        TEXT target_branch
        TEXT ci "nullable"
        INTEGER updated_at
        TEXT off_default_description "nullable"
        TEXT merge_commit "nullable"
        INTEGER ci_finished_at "nullable"
        INTEGER checks_read_at "nullable"
        TEXT head "nullable"
        TEXT base "nullable"
    }
    review_requests {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT login PK
    }
    latest_reviews {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT login PK
        TEXT state
    }
    assignees {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT login PK
    }
    pull_request_labels {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT name PK
    }
    closing_references {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER issue PK
    }
    cherry_picks {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT sha PK
    }
    committers {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER position PK
        TEXT login "nullable"
    }
    failed_checks {
        TEXT repository PK
        INTEGER number PK
        TEXT name PK
    }
    pr_reviews {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER position PK
        TEXT author "nullable"
        TEXT state
        TEXT commit_sha "nullable"
        INTEGER submitted_at
        TEXT body_html
        TEXT hidden "nullable"
    }
    review_threads {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER review PK, FK
        INTEGER position PK
        TEXT node_id
        TEXT path
        TEXT side "nullable"
        INTEGER line "nullable"
        INTEGER original_line "nullable"
        TEXT words "nullable"
        TEXT hidden "nullable"
        INTEGER replies
        TEXT resolution
        TEXT anchor
        TEXT diff_hunk "nullable"
    }
    review_thread_comments {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER review PK, FK
        INTEGER thread PK, FK
        INTEGER position PK
        TEXT author "nullable"
        INTEGER created_at
        TEXT body_html
        TEXT hidden "nullable"
        TEXT submission
    }
    review_requests }o--|| pull_requests : "repository, number"
    latest_reviews }o--|| pull_requests : "repository, number"
    assignees }o--|| pull_requests : "repository, number"
    pull_request_labels }o--|| pull_requests : "repository, number"
    closing_references }o--|| pull_requests : "repository, number"
    cherry_picks }o--|| pull_requests : "repository, number"
    committers }o--|| pull_requests : "repository, number"
    pr_reviews }o--|| issue_content : "repository, number"
    review_threads }o--|| pr_reviews : "repository, number, review"
    review_thread_comments }o--|| review_threads : "repository, number, review, thread"
```

### Issues, their timelines and the viewer

```mermaid
erDiagram
    viewer {
        INTEGER id PK
        TEXT login
    }
    issues {
        TEXT repository PK
        INTEGER number PK
        TEXT title
        TEXT author "nullable"
        INTEGER updated_at
        TEXT state "nullable"
    }
    issue_assignees {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT login PK
    }
    issue_labels {
        TEXT repository PK, FK
        INTEGER number PK, FK
        TEXT name PK
    }
    issue_content {
        TEXT repository PK
        INTEGER number PK
        TEXT body_html
        INTEGER earlier_comments
        INTEGER earlier_reviews
        TEXT head_sha "nullable"
        INTEGER opened_at "nullable"
    }
    issue_comments {
        TEXT repository PK, FK
        INTEGER number PK, FK
        INTEGER position PK
        TEXT author "nullable"
        INTEGER created_at
        TEXT body_html
        TEXT hidden "nullable"
    }
    activity {
        TEXT repository PK
        INTEGER number PK
        TEXT kind PK
        TEXT target PK
        TEXT who PK
        INTEGER at PK
        INTEGER noticed_at
    }
    issue_assignees }o--|| issues : "repository, number"
    issue_labels }o--|| issues : "repository, number"
    issue_comments }o--|| issue_content : "repository, number"
```

## State: the user's and the sync's

### Filters

```mermaid
erDiagram
    filter_matches {
        TEXT repository PK
        TEXT query PK
        INTEGER number PK
    }
    filter_problems {
        TEXT repository PK
        TEXT query PK
        TEXT problem
    }
    filter_searches {
        TEXT repository PK
        TEXT query PK
        INTEGER cut_off
    }
```

### Last seen, Marked unseen and the sync's place

```mermaid
erDiagram
    last_seen {
        TEXT repository PK
        INTEGER number PK
        INTEGER at
    }
    marked_unseen {
        TEXT repository PK
        INTEGER number PK
        INTEGER at
    }
    first_syncs {
        TEXT repository PK
        INTEGER started_at
    }
    sync_state {
        TEXT repository PK
        INTEGER started_at
        INTEGER finished_at
        INTEGER refresh_under_way
        INTEGER small
        TEXT etag "nullable"
    }
    first_sync_place {
        TEXT repository PK
        TEXT place
    }
    read_ahead {
        TEXT repository PK
        INTEGER number PK
    }
```
