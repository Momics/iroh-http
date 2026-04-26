window.BENCHMARK_DATA = {
  "lastUpdate": 1777222716672,
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
          "id": "6ad4bf15f457e74774bc6a5636531648b3e98d24",
          "message": "ci(bench): fix gh-pages push — add auto-push and skip-fetch\n\nThe second benchmark-action step in each job tried to git-fetch\ngh-pages after the first step had already committed to it locally,\ncausing a non-fast-forward rejection.\n\nFix: second step skips the fetch (reuses step 1's local branch) and\nauto-pushes both commits.  Add contents:write permission so the\nGITHUB_TOKEN can push to gh-pages.",
          "timestamp": "2026-04-26T16:54:40Z",
          "url": "https://github.com/Momics/iroh-http/commit/6ad4bf15f457e74774bc6a5636531648b3e98d24"
        },
        "date": 1777222716271,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "node/iroh/throughput-1kb",
            "value": 1.4594815272997714,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-1kb",
            "value": 1.9125838829302981,
            "unit": "MB/s"
          },
          {
            "name": "node/iroh/throughput-64kb",
            "value": 53.95123291947073,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-64kb",
            "value": 79.11698858208938,
            "unit": "MB/s"
          },
          {
            "name": "node/iroh/throughput-1mb",
            "value": 153.29097919929168,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-1mb",
            "value": 363.45246461913865,
            "unit": "MB/s"
          },
          {
            "name": "node/iroh/throughput-10mb",
            "value": 189.33533078573066,
            "unit": "MB/s"
          },
          {
            "name": "node/native/throughput-10mb",
            "value": 588.7635951548941,
            "unit": "MB/s"
          }
        ]
      }
    ]
  }
}