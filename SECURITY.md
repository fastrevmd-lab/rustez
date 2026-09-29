# Security Policy

## Reporting a vulnerability

Please **do not** open a public GitHub issue for a security vulnerability.

Instead, use GitHub's private vulnerability reporting for this repository:

https://github.com/mechubsec/rustez/security/advisories/new

Include what you'd include in a bug report — affected version, reproduction steps, and impact — but keep it in the private report, not a public issue, PR, or discussion.

## Scope

rustEZ automates Junos devices over NETCONF. Vulnerability classes we especially want to hear about: host-key verification bypass or weakening the fail-closed default, credential handling (password/key resolution, the `-p` warning path), XML payload escaping in config loads (`ConfigPayload::Text`/`Set`) that could let untrusted input reach a device unescaped, and anything that could cause a config-changing call (`load`, `commit`, `RpcExecutor::call_xml_candidate_change()` / the Python `Device.rpc.raw_xml_candidate_change()`) to fire without the caller's explicit intent.

## Response

This is a community-maintained project. There's no guaranteed SLA. A human maintainer is responsible for triaging every report and for all disclosure and fix decisions.
