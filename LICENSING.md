# Licensing

Copyright (C) 2026 h4r contributors.

h4r's original source code and original documentation are licensed under the
GNU General Public License as published by the Free Software Foundation,
either version 3 of the License, or (at your option) any later version.
The SPDX identifier is `GPL-3.0-or-later`.

h4r is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR
PURPOSE. See [LICENSE](LICENSE) for the full terms.

## Scope

This grant covers the original work contributed to this repository, including
the Rust crates and project-authored Python and Java tools. It does not grant
rights to the original Heroes IV executable, resources, or trademarks, or to
third-party material merely named or described in research notes.

External dependencies remain under their respective licenses. See
[THIRD_PARTY.md](THIRD_PARTY.md) for the current dependency inventory.

## Implementation provenance

The development workflow records observations about file formats and behavior
in `FORMATS.md`, `CLAUDE.md`, and `decomp/*.md`, then implements readers and rules
in Rust. The owner reports that other engine projects were consulted for ideas
and cross-checks, rather than as sources of copied implementation code. Existing
research notes identify those references and distinguish confirmed observations
from assumptions.

The lists in `re/` contain extracted technical identifiers and interface facts;
they are not a grant of rights over the original software. `tools/pe_info.py`
reproduces the RTTI, class, import, and source-filename lists from a local copy
of the executable. The ability-identifier table documents its executable source
in its header.

Original game resources, executables, decompiler output, and the external symbol
map are not distributed in this repository. Do not submit them as contributions.
Document the origin of any future copied or adapted code and preserve its
copyright and license notices.

Choosing GPLv3 does not itself establish whether a particular reverse-engineering
step or reimplementation is permitted. This provenance record is not a legal
clearance or a claim that the project used a separate clean-room team.
