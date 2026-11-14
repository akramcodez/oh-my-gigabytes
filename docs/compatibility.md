# Compatibility

What stays stable between omg versions, so scripts and saved state keep working.

## JSON output

Every command that prints JSON follows output schema **1**. `omg status` and `omg doctor` report it as `output_schema`.

Within one output schema:

- fields may be added, and new values may appear in fields that hold a category, kind, tier or outcome;
- fields are never removed or renamed, and never change type;
- an unknown size or time is `null`, never `0`;
- paths are display strings and are not a way to address a file.

Removing or renaming a field raises the output schema. Scripts should ignore fields they do not know.

`omg scan` prints its suggestion summary to standard error. Scripts that parse its JSON must not merge standard error into it, or must pass `--no-suggest`.

`omg clean` prints a selection with a `fingerprint`; only `--confirm` with that exact fingerprint deletes anything, and a changed selection gets a new fingerprint.

## Configuration

`config.toml` is strict TOML: unknown keys are an error rather than being ignored. Within a major version keys are not removed and never given another meaning.

## Catalog

Signed catalogs use `schema_version = 1`. Tiers stay `info` and `review` and the action stays `info`; a catalog never grants deletion. A catalog with another schema version is refused, and the current one stays active.

## Index

The index is a SQLite database at schema 13. A newer omg migrates an older index forward on open. An older omg refuses an index newer than it knows and changes nothing; `omg doctor` shows `schema` and `schema_supported`. The index is a cache: moving the state folder aside and scanning again is always safe, but loses the deletion history and trend samples.
