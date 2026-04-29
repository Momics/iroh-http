window.BENCHMARK_DATA = {
  "lastUpdate": 1777473766905,
  "repoUrl": "https://github.com/Momics/iroh-http",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "email": "momics.eu@gmail.com",
            "name": "Momics",
            "username": "Momics"
          },
          "committer": {
            "email": "momics.eu@gmail.com",
            "name": "Momics",
            "username": "Momics"
          },
          "distinct": true,
          "id": "c1bd55054f9a90012dfb0fe901771a7da994726b",
          "message": "chore: release v0.3.4",
          "timestamp": "2026-04-29T16:39:45+02:00",
          "tree_id": "5245e1f8ed6632a04638e1ae8b231b38b16e4c1e",
          "url": "https://github.com/Momics/iroh-http/commit/c1bd55054f9a90012dfb0fe901771a7da994726b"
        },
        "date": 1777473766869,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "deno/iroh/cold-connect",
            "value": 1017039.0713999999,
            "unit": "us"
          },
          {
            "name": "deno/native/cold-connect",
            "value": 838.2045000002108,
            "unit": "us"
          },
          {
            "name": "deno/iroh/warm-request",
            "value": 771.060000000216,
            "unit": "us"
          },
          {
            "name": "deno/native/warm-request",
            "value": 698.515239999906,
            "unit": "us"
          },
          {
            "name": "deno/iroh/multiplex-x8",
            "value": 3417.0080799999414,
            "unit": "us"
          },
          {
            "name": "deno/native/multiplex-x8",
            "value": 1076.505160000088,
            "unit": "us"
          },
          {
            "name": "deno/iroh/multiplex-x32",
            "value": 9697.933639999974,
            "unit": "us"
          },
          {
            "name": "deno/native/multiplex-x32",
            "value": 2984.6619200000714,
            "unit": "us"
          },
          {
            "name": "deno/iroh/serve-rps",
            "value": 637.4403599999641,
            "unit": "us"
          },
          {
            "name": "deno/native/serve-rps",
            "value": 623.3222000000387,
            "unit": "us"
          }
        ]
      }
    ]
  }
}