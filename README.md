# serverify

[![codecov](https://codecov.io/gh/autopp/serverify/graph/badge.svg?token=TMBNHI2I9F)](https://codecov.io/gh/autopp/serverify)

serverify is stub HTTP server for testing.

## Features

You can define specification of endpoint by YAML and listen as HTTP server.

After listen, you can use serverify's REST API to...

- create/delete **session** the unit of request logging
- get request information log per session

## Installation

Download executable from [releases](https://github.com/autopp/serverify/releases).


## Usage

```sh
Usage: serverify [OPTIONS] <CONFIG_PATH>

Arguments:
  <CONFIG_PATH>  

Options:
      --port <PORT>  [default: 8080]
  -h, --help         Print help
```

```sh
$ serverify example.yaml
```

## YAML Config

| field | type | requied | description |
| --- | --- | --- | --- |
| `.paths` | map | :white_check_mark: | key is path of endpoint |
| `.paths[]` | map | :white_check_mark: | key is HTTP method name such as `get` |
| `.paths[].response` | map | :white_check_mark: | response infomation for stub endpoint |
| `.paths[].status` | int | :white_check_mark: | status code of the response |
| `.paths[].headers` | map |  | headers of the response |
| `.paths[].body` | string |  | body of the response |

E.g.
```yaml
paths:
  /hello:
    get:
      response:
        status: 200
        headers:
          Content-Type: application/json
        body: '{"message": "hello"}'
```

## License

[Apache-2.0](LICENSE)
