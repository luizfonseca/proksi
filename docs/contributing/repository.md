# Repository

You are free to fork, change and create our own configurations. If you feel like it, we are also open to pull-requests and issues won't be left hanging.

Repository: [https://github.com/luizfonseca/proksi](https://github.com/luizfonseca/proksi)

## Performance checks

Every pull request is benchmarked against `main` (plain HTTP and HTTPS, on the same GitHub runner): requests per second, latency, CPU time and peak memory. The results are posted as a comment on the pull request, with a warning for changes worse than 10%. Pull requests that don't change proksi's code compare `main`'s binary with itself, without building it again.

To run it locally (Linux or macOS), with [oha](https://github.com/hatoo/oha) and any upstream (e.g. `docker run -p 8080:80 nginx:alpine`):

```bash
python3 .github/benchmark/compare.py \
  --base ./proksi-main --head ./target/release/proksi \
  --oha oha --upstream 127.0.0.1:8080 --out report.md
```
