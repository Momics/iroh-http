window.BENCHMARK_DATA = {
  "lastUpdate": 1777450117005,
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
        "date": 1777450116971,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "rust/connection_establishment",
            "value": 7059.265934285712,
            "unit": "us"
          },
          {
            "name": "rust/fetch_get_latency",
            "value": 328.18455905055276,
            "unit": "us"
          },
          {
            "name": "rust/handle_ops/alloc_body_writer",
            "value": 0.279773243781302,
            "unit": "us"
          },
          {
            "name": "rust/handle_ops/make_body_channel",
            "value": 0.21299604174515077,
            "unit": "us"
          },
          {
            "name": "rust/latency_iroh_1kb",
            "value": 346.7213987137002,
            "unit": "us"
          },
          {
            "name": "rust/multiplex/32",
            "value": 3468.9500206666658,
            "unit": "us"
          },
          {
            "name": "rust/multiplex/8",
            "value": 1213.316428802929,
            "unit": "us"
          }
        ]
      }
    ]
  }
}