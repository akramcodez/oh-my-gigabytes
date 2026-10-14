# Security policy

OMG is a development preview. It deletes files only when you confirm an exact plan, and it is not yet released for general use.

## Reporting a vulnerability

Do not open a public issue for a security problem.

- **Preferred:** GitHub private vulnerability reporting on this repository (Security tab, "Report a vulnerability"). The repository owner must enable it first.
- **Maintainer contact:** `[owner to provide before publishing]`.

Include the OMG version (`omg --version`), your OS and filesystem, and the steps to reproduce. Do not include real personal file names or contents.

## Scope

In scope: anything that deletes, moves or reads data outside what the user confirmed; path, link or race escapes; authorization bypass for permanent delete; service code that can change user files; notification or diagnostic output that leaks paths.

Out of scope for now: a hostile process running as the same user (OMG does not defend against that), unsupported filesystems, and Windows or macOS builds, which are compile checks only.
