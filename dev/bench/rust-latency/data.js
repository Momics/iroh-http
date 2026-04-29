window.BENCHMARK_DATA = {
  "lastUpdate": 1777448614029,
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
          "id": "62b6b236cb60ee4f069b1507ee26e6fdb6eabd11",
          "message": "fix: update fetch URLs to use dynamic serverId in benchmark scripts",
          "timestamp": "2026-04-29T07:35:37Z",
          "url": "https://github.com/Momics/iroh-http/commit/62b6b236cb60ee4f069b1507ee26e6fdb6eabd11"
        },
        "date": 1777448613996,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "rust/connection_establishment",
            "value": 6158.5696312499995,
            "unit": "us"
          },
          {
            "name": "rust/fetch_get_latency",
            "value": 280.9997032035379,
            "unit": "us"
          },
          {
            "name": "rust/handle_ops/alloc_body_writer",
            "value": 0.28174473796457844,
            "unit": "us"
          },
          {
            "name": "rust/handle_ops/make_body_channel",
            "value": 0.22789495701409368,
            "unit": "us"
          },
          {
            "name": "rust/latency_iroh_1kb",
            "value": 299.1162256618711,
            "unit": "us"
          },
          {
            "name": "rust/multiplex/32",
            "value": 2864.6509999999994,
            "unit": "us"
          },
          {
            "name": "rust/multiplex/8",
            "value": 961.1353232440348,
            "unit": "us"
          }
        ]
      }
    ]
  }
}