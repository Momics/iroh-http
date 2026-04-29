window.BENCHMARK_DATA = {
  "lastUpdate": 1777449948746,
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
        "date": 1777449948206,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "node/iroh/throughput-1kb",
            "value": 1.410418194220978,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-1kb",
            "value": 1.8929048812692435,
            "unit": "MB/s"
          },
          {
            "name": "node/iroh/throughput-64kb",
            "value": 52.426256975292134,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-64kb",
            "value": 79.65974861473917,
            "unit": "MB/s"
          },
          {
            "name": "node/iroh/throughput-1mb",
            "value": 147.8422264424274,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-1mb",
            "value": 353.0863439746559,
            "unit": "MB/s"
          },
          {
            "name": "node/iroh/throughput-10mb",
            "value": 191.59431409156056,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-10mb",
            "value": 568.7198982955916,
            "unit": "MB/s"
          }
        ]
      }
    ]
  }
}