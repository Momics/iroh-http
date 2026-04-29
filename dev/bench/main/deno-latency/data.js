window.BENCHMARK_DATA = {
  "lastUpdate": 1777449986807,
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
        "date": 1777449986772,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "deno/iroh/cold-connect",
            "value": 1016020.6611,
            "unit": "us"
          },
          {
            "name": "deno/native/cold-connect",
            "value": 828.6586000001989,
            "unit": "us"
          },
          {
            "name": "deno/iroh/warm-request",
            "value": 733.0497999999352,
            "unit": "us"
          },
          {
            "name": "deno/native/warm-request",
            "value": 692.5752799998008,
            "unit": "us"
          },
          {
            "name": "deno/iroh/multiplex-x8",
            "value": 3407.7153199999884,
            "unit": "us"
          },
          {
            "name": "deno/native/multiplex-x8",
            "value": 1112.8965999998763,
            "unit": "us"
          },
          {
            "name": "deno/iroh/multiplex-x32",
            "value": 8318.254559999914,
            "unit": "us"
          },
          {
            "name": "deno/native/multiplex-x32",
            "value": 2953.2432799996604,
            "unit": "us"
          },
          {
            "name": "deno/iroh/serve-rps",
            "value": 594.7050000001764,
            "unit": "us"
          },
          {
            "name": "deno/native/serve-rps",
            "value": 614.9613600003067,
            "unit": "us"
          }
        ]
      }
    ]
  }
}