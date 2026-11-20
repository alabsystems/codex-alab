<!-- Copyright 2026 Andrew Yates; SPDX-License-Identifier: Apache-2.0 -->
# Reject conflicting fetch destination mappings before transport

The next nearby issue is an order-dependent source admission gap. A head and a
lightweight tag can advertise the same raw object ID and map to one metadata
destination. The old adapter compared only the OID before replacing the stored
source and force flag. Placing the head last dropped the tag's named-reference
check; placing the tag last retained that check. Conflicting force flags for the
same source were likewise resolved by argument order.

Decision: require the complete resolved source/OID/force tuple to agree for every
repeated destination. Refuse conflicting mappings before transfer, including
when the advertised object IDs match. This avoids choosing an arbitrary source's
admission policy. Exact duplicates and overlapping wildcard/literal mappings
that resolve to the same tuple remain valid. Multiple heads sharing an object
but using distinct destinations keep the existing grouped transfer behavior.
Normal release callers use one main destination, disjoint main/tag namespaces,
or unique identity mappings; none depends on ambiguous fan-in.

The branch starts from pinned main
f00c052b52eacce6c04cb0779f674256b998f4ae. The GitHub issue list was unavailable:
the actual selected Agh returned typed 403 for issues=read. That operation is
recorded and deferred, with no alternate identity or permission workaround.
The nearby certification notes remain deferred until their existing prerequisites
are available. This task proceeds with the concrete source-backed finding.

Tests-only commit dee2ed342f4359fc7a5a80a8eef08144be7cdc5b pins six regression
bodies before the production change. The original implementation completed two
passing controls and four assertion failures, with zero errors or skips. Head-last
accepted a fetch after the actual tag moved and installed the old advertised OID;
forced-last replaced a real unrelated destination despite its unforced mapping.
The reverse orders refused after one delegated attempt and preserved their
destinations. Those two late refusals are not unsafe metadata updates, and a
delegated attempt does not by itself prove a completed transfer. Caller HEAD
remained unchanged. The duplicate/wildcard-literal and separate-destination
head/tag controls passed. Source/tool bookends matched, and all original raw
results, failures and nonzero collector/guardian statuses remain retained.

The independently approved source fix is committed at
73ad4f53e8dc28dc5d35d8bbba6626d1d6710163. Before verification, current main
026bea9f6d3969e23e4014d038c051871c4c8985 was merged by its fixed full OID into
2072a1440599c974cb59f0611dac1dc9e85499af, retaining its disjoint shell-state work.
The complete 76-method GitHub transport selection passed on that merged source:
all exact AST and named result identities finished, with no failures, errors or
skips. This includes the six new cases and the existing snapshot, CLI, cleanup,
graft and publication controls. Original process, guardian, collector and source
statuses were all zero; raw output matched the maintained named result record.
The before/after source and tool books were identical. The six baseline methods
are included in the 76, not six additional distinct tests. No further execution
was warranted after the passing selection and unchanged source checks.

Production github.py SHA-256:
61389ac63ed816a453bb9d999c0c23f27a7d796008d94e87d6cce68b8d63c3fa.
Frozen regression module SHA-256:
ff86cf66dd597d564df6112009bf2249287b025d96774019054c57f7cfb379ad.
Baseline summary SHA-256:
c59403f783e7802e2a1fc8fa3b85c6dbfdfeb5186526752bd4933aaf49cf47e1.
Passing summary SHA-256:
6477f774d40cd183cbdcf102addcd534371bc351fba98691b8e260757836dd4a.

The finite evidence packet is
codex-github-destination-policy-evidence-8dz6n89v.noindex: 21 payload files,
1,162,412 bytes, index SHA-256
59cea441c8d78041620e77eb596ce5bc921005b24c5a323f5ec3fcdf0a75d39d.
It retains both original outcomes, raw results, typed completion receipts,
original process handles, plans, exact identities and source/tool bookends.
Payloads are sealed read-only; there is no live HEAD, source cache, binary or
complete source-copy dependency. Independent source, fixture and outcome review
approved the minimal change and the distinction between successful unsafe
updates and late refusals in the baseline.

The controlled fixtures use owned local Git repositories. No application build,
certification, package promotion, installed-app change or new native runtime
authority is claimed. The existing application certification prerequisites are
unchanged; this passing Python selection does not qualify those deferred cases.
