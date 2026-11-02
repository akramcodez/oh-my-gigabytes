# Security policy

OMG is a development preview. The beta release (`0.1.0-beta.1`) includes an unqualified permanent-delete feature on Linux and Windows, which deletes files only after you confirm an exact plan; the macOS build is analysis only and cannot delete. A build made with `--no-default-features` compiles delete out.

## Reporting a vulnerability

Do not open a public issue for a security problem.

- **Preferred:** GitHub private vulnerability reporting on this repository (Security tab, "Report a vulnerability"). The repository owner must enable it first.
- **Maintainer contact:** `[owner to provide before publishing]`.

Include the OMG version (`omg --version`), your OS and filesystem, and the steps to reproduce. Do not include real personal file names or contents.

## Scope

In scope: anything that deletes, moves or reads data outside what the user confirmed; path, link or race escapes; authorization bypass for permanent delete; service code that can change user files; notification or diagnostic output that leaks paths.

In scope on every supported platform (Linux, Windows and macOS): the scanner following a link or crossing a mount it should not, reading file contents, and the npm launcher running a binary that does not match its signed manifest.

Out of scope for now: a hostile process running as the same user (OMG does not defend against that) and unsupported filesystems.
