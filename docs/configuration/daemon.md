# Daemon

!> Daemon mode only works if every path provided is **absolute**. _Any path defined using  `./` won't work as the daemon will not be able to find it since it runs on a different path scope._

## Command

Daemon mode is a mode that allows Proksi to run as a **background** process.

This is useful for running Proksi on a server, where you don't want to have to manually start and stop Proksi every time you want to use it.

To run Proksi as a daemon, you need to use the `-d` or `--daemon` flag.

For example, to run Proksi as a daemon, you can use the following command:

```bash
proksi -d -c /absolute-path-to-config-folder
```



## Caveats
