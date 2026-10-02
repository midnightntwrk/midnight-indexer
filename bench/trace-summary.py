# This file is part of midnight-indexer.
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# You may not use this file except in compliance with the License.
# You may obtain a copy of the License at
# http://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Summarize chain-indexer console spans from a sync-bench run with tracing enabled.

Usage: uv run bench/trace-summary.py <indexer.log>   (stdlib only, so python3 works too)

Run the bench with
  WRAP="env APP__TELEMETRY__TRACING__ENABLED=true APP__TELEMETRY__TRACING__CONSOLE_REPORTER_ENABLED=true"
Times are inclusive (a span's time contains its children's), as a share of the per-block root span.
"""

import json
import re
import sys
from collections import defaultdict

ROOT = "get-and-index-block"

text = open(sys.argv[1], errors="replace").read()
spans = []
for record in text.split("SpanRecord {")[1:]:
    name = re.search(r'\n    name: "([^"]*)"', record)
    duration = re.search(r"\n    duration_ns: (\d+)", record)
    if not name or not duration:
        continue
    name, duration = name.group(1), int(duration.group(1))
    if name == "method_call":
        method = re.search(r'"method",\s*"([^"]*)"', record)
        name = "rpc " + (method.group(1) if method else "?")
        send = re.search(r'"send",\s*"((?:[^"\\]|\\.)*)"', record)
        # The runtime function is params[0] for state_call, params[1] for archive_v1_call.
        fn_index = {"rpc state_call": 0, "rpc archive_v1_call": 1}.get(name)
        if fn_index is not None and send:
            try:
                params = json.loads(json.loads(f'"{send.group(1)}"'))["params"]
                name += " " + params[fn_index]
            except (ValueError, KeyError, IndexError):
                pass
    spans.append((name, duration))

totals = defaultdict(lambda: [0, 0])
for name, duration in spans:
    totals[name][0] += 1
    totals[name][1] += duration

blocks, root_ns = totals[ROOT]
if not blocks:
    sys.exit(f"no {ROOT} spans found")
print(f"blocks: {blocks}, mean {ROOT}: {root_ns / blocks / 1e6:.1f} ms\n")
print(f"{'span':<80} {'per block':>9} {'ms/block':>9} {'% root':>7}")
for name, (count, ns) in sorted(totals.items(), key=lambda kv: -kv[1][1]):
    if ns / root_ns < 0.001:
        continue
    print(f"{name[:80]:<80} {count / blocks:>9.2f} {ns / blocks / 1e6:>9.2f} {100 * ns / root_ns:>6.1f}%")
