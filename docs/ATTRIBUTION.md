# Attribution and license

## Upstream work

**alttab-rs is a derivative work of AltTab and is licensed under GPL-3.0.**

AltTab is the macOS window switcher developed by **lwouis**
and the AltTab contributors:

- Repository: <https://github.com/lwouis/alt-tab-macos>
- Website: <https://alt-tab.app/>
- Reference revision: `c500294bdb01186011a5c64a412b04cef9931bef`
- Revision link: <https://github.com/lwouis/alt-tab-macos/tree/c500294bdb01186011a5c64a412b04cef9931bef>
- Upstream license: [GNU General Public License, version 3](https://github.com/lwouis/alt-tab-macos/blob/c500294bdb01186011a5c64a412b04cef9931bef/LICENCE.md).

The reference source was inspected read-only at `/workspace/alttab-src`.
The revision was checked with `git rev-parse HEAD`.
Its license text identifies GPL version 3 in `LICENCE.md:1-6`.
The root [LICENSE](../LICENSE) is the supplied copy of that license.
Credit for upstream behavior, design, and any subsequently adapted source
belongs to the upstream authors; a Rust translation does not remove their rights.

## This repository

Phase 1 contains an evidence-backed specification and a proposed Rust
architecture, not an implemented or runtime-validated replacement.
Source citations in [SPEC.md](SPEC.md) refer to the upstream revision above;
[ARCHITECTURE.md](ARCHITECTURE.md) distinguishes new design decisions from
observed upstream behavior.

Preserve applicable upstream copyright and license notices when adapting code
or assets. Mark modified files appropriately and provide corresponding source
and license notices when distributing covered binaries, as required by GPL-3.0.
Do not imply endorsement by lwouis or the AltTab project.
No upstream logos or application artwork are copied in this phase.

The Free Software Foundation copyright in the license text concerns the license
itself, not authorship of AltTab or alttab-rs. Third-party Rust dependencies retain
their own licenses; their notices must accompany distribution where required.
The dependency plan is not a substitute for a distribution-time license audit.
