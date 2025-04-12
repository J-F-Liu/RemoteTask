# Remote Task

A simple web server that serve APIs to run [just](https://github.com/casey/just) tasks remotely.

- `POST /run` - Shedule a new task
- `POST /reset/{id}` - Reset task status so it will be run again
- `POST /canel/{id}` - Delete a task from shedule
- `GET /list/{page}` - Get a list of recent tasks

See `test.rest` for how to use the APIs.

An example web page is created for my own need.

### Dependencies

- Axum: Web server framework
- SeaORM: Sqlite database operations
- Dioxus: Create web pages


### How to use
1. Install [just](https://github.com/casey/just)
2. Edit `.env` file to set environment variables
3. Inside `WORK_DIR` create a `justfile` and define your tasks
4. Start the server
5. Use [xh](https://github.com/ducaale/xh) or VSCode REST client to call the APIs.

### How to build
1. Run `just build --release` to build the server
2. Run `just run --release` to run the server