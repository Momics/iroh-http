window.BENCHMARK_DATA = {
  "lastUpdate": 1777449949192,
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
        "date": 1777449949159,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "node/iroh/cold-connect",
            "value": 98924.75239999997,
            "unit": "us"
          },
          {
            "name": "node/native/cold-connect",
            "value": 1303.5353000000214,
            "unit": "us"
          },
          {
            "name": "node/iroh/warm-request",
            "value": 771.4982000000055,
            "unit": "us"
          },
          {
            "name": "node/native/warm-request",
            "value": 817.5433599999815,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x8",
            "value": 2881.640120000011,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x8",
            "value": 2837.7913199999784,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x32",
            "value": 8321.024360000029,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x32",
            "value": 7011.240800000032,
            "unit": "us"
          },
          {
            "name": "node/iroh/serve-rps",
            "value": 590.7819599999493,
            "unit": "us"
          },
          {
            "name": "node/native/serve-rps",
            "value": 238.8221600000179,
            "unit": "us"
          }
        ]
      }
    ]
  }
}