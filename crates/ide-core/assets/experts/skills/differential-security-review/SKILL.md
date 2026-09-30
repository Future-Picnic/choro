---
name: differential-security-review
description: "Trace the security consequences of the assigned change and report evidence-backed vulnerabilities."
license: CC-BY-SA-4.0
---

# Differential Security Review · Choro edition

Start with the assigned change and identify affected trust boundaries, callers, and assets. Read relevant history when it explains a security invariant or a previously fixed regression. Scale the review to the risk and changed surface; do not expand every assignment into a repository-wide audit.

Trace attacker-controlled input to sensitive operations. Check authorization for the actual object and actor, including already-authenticated users, tenant boundaries, path containment, command/query construction, secret exposure, and failure defaults. Investigate relevant races and partial writes. Verify whether framework protections or earlier validation actually mitigate the suspected path.

For each finding provide a plausible trigger, location, severity, confidence, concrete impact, and remediation. Redact credentials and personal data. Distinguish confirmed paths from hypotheses; an old package version alone is not evidence of a current vulnerability. Use authoritative current advisories when dependency analysis is relevant.

Return a scoped report with unresolved verification gaps. Demonstrations use isolated fixtures and permitted operations. Native reviewer agents, comprehensive report files, and automatic deployment are not part of this instruction edition. Fixes require an implementation assignment; existing authorization remains effective.
