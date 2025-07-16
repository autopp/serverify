# serverify

[![codecov](https://codecov.io/gh/autopp/serverify/graph/badge.svg?token=TMBNHI2I9F)](https://codecov.io/gh/autopp/serverify)

serverify is a stub HTTP server for testing. It allows you to quickly mock HTTP endpoints for integration testing, API development, and debugging.

## Features

- Define HTTP endpoints via YAML configuration
- Session-based request logging to track API interactions
- Support responses with paging
- RESTful session management API
- In-memory request history tracking
- Support for custom headers, query parameters, and response bodies

## Installation

Download the executable from [releases](https://github.com/autopp/serverify/releases).

Or build from source:
```bash
git clone https://github.com/autopp/serverify.git
cd serverify
cargo build --release --features cli
```

## Quick Start

1. Create a configuration file `example.yaml`:
```yaml
paths:
  /hello:
    get:
      response:
        type: static
        status: 200
        headers:
          Content-Type: application/json
        body: '{"message": "Hello, World!"}'
```

2. Start the server:
```bash
serverify example.yaml
```

3. Test your endpoint:
```bash
curl http://localhost:8080/mock/default/hello
# {"message": "Hello, World!"}
```

## Usage

```sh
Usage: serverify [OPTIONS] <CONFIG_PATH>

Arguments:
  <CONFIG_PATH>  Path to YAML configuration file

Options:
      --port <PORT>  Port to listen on [default: 8080]
  -h, --help         Print help
```

## Configuration

### Basic Structure

The configuration file uses YAML format with the following structure:

```yaml
paths:
  <endpoint_path>:
    <http_method>:
      response:
        type: <static|paging>
        # ... response configuration
```

### Response Types

#### Static Response

Static responses return fixed content for every request:

```yaml
paths:
  /api/user:
    get:
      response:
        type: static
        status: 200
        headers:
          Content-Type: application/json
          X-Custom-Header: value
        body: '{"id": 1, "name": "John Doe"}'
```

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `type` | string | ✓ | Must be "static" |
| `status` | integer | ✓ | HTTP status code |
| `headers` | map | | Response headers |
| `body` | string | | Response body |

#### Paging Response

Paging responses support pagination through query parameters:

```yaml
paths:
  /api/users:
    get:
      response:
        type: paging
        status: 200
        page_param: page
        per_page_param: limit
        default_per_page: 10
        page_origin: 1
        template:
          data: $_contents
          page: $_page
          total: $_total
        items:
          - id: 1
            name: Alice
          - id: 2
            name: Bob
          - id: 3
            name: Charlie
```

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `type` | string | ✓ | Must be "paging" |
| `status` | integer | ✓ | HTTP status code |
| `page_param` | string | ✓ | Query parameter name for page number |
| `per_page_param` | string | ✓ | Query parameter name for items per page |
| `default_per_page` | integer | ✓ | Default items per page |
| `page_origin` | integer | ✓ | Page numbering origin (0 or 1) |
| `template` | object | ✓ | Response template with placeholders |
| `items` | array | ✓ | Array of items to paginate |

Template placeholders:
- `$_contents`: Current page items
- `$_page`: Current page number
- `$_total`: Total number of items

## Session Management

serverify tracks requests by session. Each mock endpoint is accessed via:
```
/mock/{session_id}/{endpoint_path}
```

The default session ID is `default`.

### Session API Endpoints

#### Create Session
```bash
POST /session
Content-Type: application/json

{
  "session": "test-session-1"
}

# Response: 201 Created
{
  "session": "test-session-1"
}
```

#### Get Session History
```bash
GET /session/{session_id}

# Response: 200 OK
{
  "histories": [
    {
      "path": "/api/users",
      "method": "get",
      "headers": {
        "user-agent": "curl/7.68.0",
        "accept": "*/*"
      },
      "query": {
        "page": "2"
      },
      "body": "",
      "timestamp": "2024-01-01T12:00:00Z"
    }
  ]
}
```

#### Delete Session
```bash
DELETE /session/{session_id}

# Response: 204 No Content
```

## Complete Examples

### Example 1: Simple REST API Mock

```yaml
paths:
  /api/health:
    get:
      response:
        type: static
        status: 200
        body: '{"status": "healthy"}'
  
  /api/users:
    get:
      response:
        type: paging
        status: 200
        page_param: page
        per_page_param: per_page
        default_per_page: 20
        page_origin: 1
        template:
          users: $_contents
          pagination:
            current_page: $_page
            total_items: $_total
        items:
          - id: 1
            name: Alice
            email: alice@example.com
          - id: 2
            name: Bob
            email: bob@example.com
    
    post:
      response:
        type: static
        status: 201
        headers:
          Content-Type: application/json
          Location: /api/users/3
        body: '{"id": 3, "name": "New User", "email": "new@example.com"}'
  
  /api/users/{id}:
    get:
      response:
        type: static
        status: 200
        body: '{"id": 1, "name": "Alice", "email": "alice@example.com"}'
    
    put:
      response:
        type: static
        status: 200
        body: '{"id": 1, "name": "Alice Updated", "email": "alice@example.com"}'
    
    delete:
      response:
        type: static
        status: 204
```

### Example 2: Testing with Sessions

```bash
# Create a test session
curl -X POST http://localhost:8080/session \
  -H "Content-Type: application/json" \
  -d '{"session": "integration-test-1"}'

# Make requests using the session
curl http://localhost:8080/mock/integration-test-1/api/users?page=1&per_page=5
curl -X POST http://localhost:8080/mock/integration-test-1/api/users \
  -H "Content-Type: application/json" \
  -d '{"name": "Test User"}'

# Check request history
curl http://localhost:8080/session/integration-test-1

# Clean up
curl -X DELETE http://localhost:8080/session/integration-test-1
```

### Example 3: Error Responses

```yaml
paths:
  /api/protected:
    get:
      response:
        type: static
        status: 401
        headers:
          WWW-Authenticate: Bearer
        body: '{"error": "Unauthorized"}'
  
  /api/not-found:
    get:
      response:
        type: static
        status: 404
        body: '{"error": "Resource not found"}'
  
  /api/server-error:
    get:
      response:
        type: static
        status: 500
        body: '{"error": "Internal server error"}'
```

## Health Check

serverify provides a health check endpoint:
```bash
GET /health

# Response: 200 OK
{"status": "ok"}
```

## Development

### Building
```bash
# Build debug version
cargo build --features cli

# Build release version
cargo build --release --features cli

# Run tests
cargo test

# Run E2E tests
cargo build --features cli && ./e2e/run.sh
```

### Running with Cargo
```bash
cargo run --features cli -- example.yaml --port 3000
```

## License

[Apache-2.0](LICENSE)