window.BENCHMARK_DATA = {
  "lastUpdate": 1777473825524,
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
        "date": 1777473825498,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "node/iroh/cold-connect",
            "value": 95829.1748,
            "unit": "us"
          },
          {
            "name": "node/native/cold-connect",
            "value": 1480.9093000000075,
            "unit": "us"
          },
          {
            "name": "node/iroh/warm-request",
            "value": 656.5577600000051,
            "unit": "us"
          },
          {
            "name": "node/native/warm-request",
            "value": 779.2765600000075,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x8",
            "value": 2260.93772000002,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x8",
            "value": 2222.6309599999877,
            "unit": "us"
          },
          {
            "name": "node/iroh/multiplex-x32",
            "value": 5883.545919999979,
            "unit": "us"
          },
          {
            "name": "node/native/multiplex-x32",
            "value": 5121.606319999955,
            "unit": "us"
          },
          {
            "name": "node/iroh/serve-rps",
            "value": 469.28783999996085,
            "unit": "us"
          },
          {
            "name": "node/native/serve-rps",
            "value": 189.62344000001394,
            "unit": "us"
          }
        ]
      }
    ]
  }
}