window.BENCHMARK_DATA = {
  "lastUpdate": 1777473994746,
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
        "date": 1777473994711,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "rust/connection_establishment",
            "value": 7402.936378571432,
            "unit": "us"
          },
          {
            "name": "rust/fetch_get_latency",
            "value": 337.46621200526425,
            "unit": "us"
          },
          {
            "name": "rust/handle_ops/alloc_body_writer",
            "value": 0.2961564667626013,
            "unit": "us"
          },
          {
            "name": "rust/handle_ops/make_body_channel",
            "value": 0.23010858520273672,
            "unit": "us"
          },
          {
            "name": "rust/latency_iroh_1kb",
            "value": 363.6290227153806,
            "unit": "us"
          },
          {
            "name": "rust/multiplex/32",
            "value": 3683.7570971428577,
            "unit": "us"
          },
          {
            "name": "rust/multiplex/8",
            "value": 1303.2134137929463,
            "unit": "us"
          }
        ]
      }
    ]
  }
}