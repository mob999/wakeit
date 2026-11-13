# Workspaces

A Gateway can host independent workspaces. An account has a separate Admin, Operator or Viewer membership in each workspace. A workspace creator becomes its Admin; invitations grant only the selected workspace role. Instance administrators manage global accounts and do not automatically gain access to workspace data.

Business requests use `/api/v1/workspaces/{workspace-id}/…`. Browser sessions identify an account; API Keys identify an account **and one fixed workspace**. A resource from another workspace returns 404. Existing unscoped business APIs return an upgrade-required response. Runner uses the independently released CLI version; product version numbers do not need to match.

## Self-hosted authentication

The default is password authentication with public registration disabled:

```yaml
auth:
  mode: local
  allow_registration: false
```

A valid invitation permits a new local account to register and join that workspace. Existing accounts explicitly accept an invitation. Set `allow_registration: true` to permit standalone account registration; registration does not create a workspace or grant access to Default.

Before first startup, `wakeit-gateway bootstrap` creates the first local account as an instance administrator and Default workspace Admin. Workspace Admins manage memberships, not other users' global passwords or accounts.

## GitHub authentication

Register a GitHub OAuth App with the Gateway's public origin as homepage and the exact callback `https://wakeit.example.com/api/v1/auth/github/callback`. For a local preview, loopback HTTP is supported. Production connections require HTTPS.

```yaml
auth:
  mode: github
  github:
    client_id_env: WAKEIT_GITHUB_CLIENT_ID
    client_secret_env: WAKEIT_GITHUB_CLIENT_SECRET
  demo_workspace: true
```

Provide those variables through the service's private environment file or secret manager. Do not commit their values. All Gateway replicas use the same OAuth App and public URL. `public_url` must be reachable by the browser and must match the registered callback origin.

GitHub login uses authorization code, S256 PKCE, a single-use state and a ten-minute browser-bound transaction shared through PostgreSQL. The transient cookie is HttpOnly and SameSite=Lax for the cross-site callback; the resulting Wakeit session remains SameSite=Strict. No repository or email scope is requested, and GitHub access tokens are not stored. Accounts bind to GitHub's immutable user ID; identical usernames never merge accounts.

When running independent Gateways on the same hostname, set a distinct `auth.session_cookie` (for example, `wakeit_workspace_preview`). Cookies are not separated by TCP port; the default remains `wakeit_session` for compatibility. Use the same cookie name on replicas of one service.

First login creates only the global account. The user can then create a workspace, accept an invitation or open the demo. An operator can explicitly designate a GitHub account as instance administrator after obtaining its account UUID:

```sh
wakeit-gateway grant-instance-admin --config gateway.yaml --user-id ACCOUNT_UUID
```

`demo_workspace: true` requires GitHub or GitLab mode and creates synthetic, non-executable examples once. Externally authenticated sessions receive read-only demo access without membership rows. No device registration, task mutation, API Key issuance, directory/model/session query or continuation is allowed there. Do not seed the demo from real execution data.

## Self-hosted GitLab authentication

Set `auth.mode: gitlab` with the canonical HTTPS origin of your GitLab instance (loopback HTTP is allowed only for local development). You can enable OAuth 2.0 browser redirects, Personal Access Token sign-in, or both:

```yaml
auth:
  mode: gitlab
  gitlab:
    url: https://gitlab.example.com
    client_id_env: WAKEIT_GITLAB_CLIENT_ID
    client_secret_env: WAKEIT_GITLAB_CLIENT_SECRET
    allow_token: true
```

For OAuth 2.0, register a GitLab Application with scope `read_user` and callback `https://wakeit.example.com/api/v1/auth/gitlab/callback`. `client_secret_env` may be omitted for public PKCE applications. When `allow_token: true` is set, users can also sign in directly with a `read_user` Personal Access Token without registering an OAuth Application.

GitLab access tokens and Personal Access Tokens are used once to verify `/api/v4/user` and are never stored or returned. Accounts bind to `{gitlab_url}:{immutable_user_id}`, so username changes preserve the account and accounts from different GitLab instances never collide. Use `wakeit-gateway grant-instance-admin` to designate an instance administrator by Wakeit account UUID.

## CLI and Runners

```sh
wakeit login --gateway https://wakeit.example.com --workspace WORKSPACE_UUID
wakeit setup --gateway https://wakeit.example.com --workspace WORKSPACE_UUID
wakeit connection list
wakeit connection use CONNECTION_NAME
```

Connections and credentials are separated by Gateway, workspace and account. Browser device authorization fixes the workspace from request through approval, credential exchange and device registration. Operator and Admin can register Runners; Viewer can authorize CLI-only access. Only workspace Admin can revoke a device.

A Runner instance belongs permanently to one workspace. To use one machine in two workspaces, register independent instances with separate configuration, Journal, keys and service names. This does not isolate native Agent files belonging to the same operating-system user; use separate system users or containers when that isolation is required.

## Upgrading an existing service

Back up PostgreSQL and stop Gateway replicas before explicit migration. Keep the published `0001_initial.sql`; `0002_workspaces.sql` moves existing business data, roles and API Keys into Default while preserving IDs and device credentials. Existing Web sessions remain global account sessions. Outstanding legacy invitations and enrollment grants expire because their original issuer cannot be verified; create new scoped grants after upgrading. Pending device authorizations must be requested again.

Existing workspace Admins are not automatically promoted to instance administrators. An operator must explicitly run `gateway grant-instance-admin` for the intended global account. Old Runners can continue using their device identity and protocol version 1. Upgrade CLI clients for scoped business APIs; an authenticated legacy profile can discover its fixed Default workspace without rotating credentials or moving Runner files.

For acceptance, use a separate database, NATS instance, configuration directory and ports. `scripts/integration_workspaces.py` tests real multi-workspace execution and CLI isolation; `scripts/workspace_migration_checks.py` checks fresh and populated schemas; `scripts/github_auth_checks.py` tests OAuth callbacks against a mock provider and a disposable database. A real GitHub browser login remains a separate acceptance step.
