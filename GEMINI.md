# Senior Developer & Security Guidelines (Coding Agent Instructions)

> **CRITICAL INSTRUCTION FOR ALL AI CODING AGENTS**:
> Whenever you analyze, plan, edit, or refactor code in this repository (**git-axolotl**), you must **strictly adhere** to all architectural best practices, security standards, UI/UX conventions, documentation maintenance, and commit conventions detailed in this document.

---

## 1. Project Overview & Architecture

**git-axolotl** is a production-grade application engineered with **Rust**.

> A high-performance, memory-safe Rust system application.

### Project Structure & Separation of Concerns:
```text
git-axolotl/
├── .github                  # CI/CD & Container Orchestration
│   └── workflows
│       ├── release.yml
│       └── axolotl.yml
├── src                      # Application Source Code
│   ├── coat.rs
│   ├── collect.rs
│   ├── history.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── output.rs
│   └── render.rs
├── .gitignore
├── action.yml
├── Cargo.toml
└── README.md
```

---

## 2. Professional Mindset & Clean Code Standards

1. **Idiomatic Rust Engineering:**
   - Write memory-safe, zero-cost abstractions with explicit ownership, borrowing, and lifetime management.
   - Never use `.unwrap()` or `.expect()` in production code paths; propagate structured errors via `Result<T, E>` and `?`.

2. **DRY & Single Responsibility Principles:**
   - Decompose monolithic routines and large files into focused, reusable modules, services, and utility functions.
   - Decouple external API integrations and persistence layers behind clear domain interfaces.

---

## 3. Framework & Technical Guidelines

Follow standard idiomatic software design patterns, modular architecture, and clean separation of concerns.

---

## 4. Zero-Vulnerability & Cybersecurity Principles

1. **Zero Exposed Secrets & Environment Validation:**
   - Never hardcode private API keys, database credentials, or sensitive tokens in client-facing or version-controlled files.
   - Validate all runtime environment variables at application boot using schema parsers (e.g. Zod / envalid).

2. **Network Resilience & Latency Handling:**
   - All network calls must specify explicit timeout thresholds (e.g. `AbortSignal.timeout(10000)`).
   - Implement automatic exponential backoff retry with jitter for idempotent network requests to handle unstable 3G or high-latency connections.
   - Handle offline and degraded connection states gracefully (`navigator.onLine`, `offline` event listeners) without unhandled promise rejections or freezing UI state.

3. **Injection Defense & Cross-Platform Path Safety:**
   - Never construct shell commands via string concatenation (e.g., avoid `exec(`cmd ${input}`)`). Use parameterized execution APIs (`execFile` or `spawn` with array arguments) to eliminate Command Injection (CWE-78).
   - Normalize all file system paths using `node:path` (`path.join`, `path.resolve`) to ensure seamless cross-platform reliability on Windows, macOS, and Linux.
   - Parameterize all SQL/database queries and sanitize any HTML rendered dynamically (e.g. via DOMPurify) to prevent SQLi and XSS.

4. **Resource Management & Concurrency Safety:**
   - Prevent memory leaks: always unregister event listeners, clear intervals/timers, abort pending network requests, and tear down WebSockets/workers when components unmount.
   - Offload heavy compute operations away from the main UI thread to Web Workers or Worker Threads to maintain responsive 60fps rendering.

5. **Clean Build Artifacts & Git Hygiene:**
   - Keep build artifacts (`dist`, `dist-electron`, `release`, `build`, `node_modules`, `.next`, `.turbo`) ignored in `.gitignore`.

---

## 5. Continuous Documentation Maintenance (README Sync)

1. **Keep Documentation Synchronized:**
   - Whenever adding new features, components, architecture changes, settings options, or build requirements, **you must update `README.md`** to reflect the changes.
   - Keep technology stack listings, installation/build instructions, and environment variable notes completely accurate.

---

## 6. Standard Git Commit Message Conventions

All commit messages in this project must follow the standard **Conventional Commits** specification:

### Format:
```text
<type>(<scope>): <short description in imperative mood>
```

### Commit Types:
* `feat(scope):` -> A new feature or user-facing capability (e.g. `feat(models): add animated cyber samurai 3D model`).
* `fix(scope):` -> A bug fix or error correction (e.g. `fix(tray): update mute icon on tray menu toggle`).
* `perf(scope):` -> Performance improvement (e.g. `perf(three): dispose unused textures on model swap`).
* `refactor(scope):` -> Code restructuring without changing functional behavior (e.g. `refactor(timers): extract interval calculation helper`).
* `security(scope):` -> Security hardening or IPC sanitization (e.g. `security(ipc): validate payload structure in main process`).
* `style(scope):` -> Styling, theme, or layout tweaks (e.g. `style(settings): refine slider contrast for dark theme`).
* `docs(scope):` -> Documentation updates (e.g. `docs(readme): document new stretch break intervals`).
* `chore(scope):` -> Maintenance, dependencies, build configuration (e.g. `chore(deps): update electron to latest patch release`).

---

## 7. Verification & Quality Assurance Runbook

Before completing any coding task, the agent must run and verify all of the following:

1. **Rust Cargo Verification:**
   ```bash
   cargo check && cargo clippy -- -D warnings
   ```
   *Must pass cleanly with zero warnings or errors.*
