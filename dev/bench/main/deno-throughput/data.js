window.BENCHMARK_DATA = {
  "lastUpdate": 1777449986404,
  "repoUrl": "https://github.com/Momics/iroh-http",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "name": "Momics",
            "username": "Momics",
            "email": "momics.eu@gmail.com"
          },
          "committer": {
            "name": "Momics",
            "username": "Momics",
            "email": "momics.eu@gmail.com"
          },
          "id": "93bb084ff64d2b8a1fcd434879135542767c0097",
          "message": "ci(bench): split release/main data paths and disable fail-on-alert\n\nTag pushes write to dev/bench/releases/<metric> (permanent per-release\nhistory). Manual workflow_dispatch writes to dev/bench/main/<metric>\n(dynamic main snapshot). Both series are visible on the GitHub Pages\nchart side by side.\n\nRemove fail-on-alert so regressions post a comment but never block the\nworkflow — performance changes between releases may be intentional.",
          "timestamp": "2026-04-29T08:01:41Z",
          "url": "https://github.com/Momics/iroh-http/commit/93bb084ff64d2b8a1fcd434879135542767c0097"
        },
        "date": 1777449986124,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "deno/iroh/throughput-1kb",
            "value": 1.020961032416419,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-1kb",
            "value": 1.468242584759756,
            "unit": "MB/s"
          },
          {
            "name": "deno/iroh/throughput-64kb",
            "value": 34.60494682642653,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-64kb",
            "value": 83.05034647539489,
            "unit": "MB/s"
          },
          {
            "name": "deno/iroh/throughput-1mb",
            "value": 74.63481728735404,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-1mb",
            "value": 645.4126764781224,
            "unit": "MB/s"
          },
          {
            "name": "deno/iroh/throughput-10mb",
            "value": 77.81877709334151,
            "unit": "MB/s"
          },
          {
            "name": "deno/native/throughput-10mb",
            "value": 803.3321381625518,
            "unit": "MB/s"
          }
        ]
      }
    ]
  }
}