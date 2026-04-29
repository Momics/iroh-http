window.BENCHMARK_DATA = {
  "lastUpdate": 1777450116593,
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
        "date": 1777450116266,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "rust/throughput_post_body/1024",
            "value": 2.8493188298434564,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_post_body/1048576",
            "value": 223.1800489980639,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_post_body/10485760",
            "value": 239.19729387047053,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_post_body/65536",
            "value": 103.71651387423837,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/1024",
            "value": 2.787653561636185,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/1048576",
            "value": 221.9361281816167,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/10485760",
            "value": 237.87493288852664,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/65536",
            "value": 106.15813704938347,
            "unit": "MB/s"
          }
        ]
      }
    ]
  }
}