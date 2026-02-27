# Security Policy

DirectScreenAPI targets privileged runtime scenarios.

## Supported Scope

Security issues accepted for:
- memory safety and UB in Rust/C boundaries
- privilege escalation through public APIs
- unsafe process lifecycle handling
- unexpected persistent side effects

## Reporting

Please report privately to project maintainers before public disclosure.
Include:
- affected version
- minimal reproduction
- impact scope
- proposed mitigation (if available)

## Hard Rules

- no system partition writes in baseline runtime
- no hidden background persistence
- all privileged operations must be explicit and observable
