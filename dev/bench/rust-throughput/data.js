window.BENCHMARK_DATA = {
  "lastUpdate": 1777448613624,
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
        "date": 1777448613301,
        "tool": "customBiggerIsBetter",
        "benches": [
          {
            "name": "rust/throughput_post_body/1024",
            "value": 3.3950875229832618,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_post_body/1048576",
            "value": 261.0828819819497,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_post_body/10485760",
            "value": 270.4177775076115,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_post_body/65536",
            "value": 120.3247819576629,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/1024",
            "value": 3.1640030377865247,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/1048576",
            "value": 259.9899859137146,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/10485760",
            "value": 274.3964117292501,
            "unit": "MB/s"
          },
          {
            "name": "rust/throughput_response_body/65536",
            "value": 121.45467511808698,
            "unit": "MB/s"
          }
        ]
      }
    ]
  }
}