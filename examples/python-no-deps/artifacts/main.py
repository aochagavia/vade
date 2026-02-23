from http.server import BaseHTTPRequestHandler, HTTPServer
import os

class HelloWorldHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        api_key = os.environ.get('API_KEY', 'NOT_SET')
        quoted_value = os.environ.get('QUOTED_VALUE', 'NOT_SET')

        response = f'Hello world\nAPI_KEY: {api_key}\nQUOTED_VALUE: {quoted_value}'

        self.send_response(200)
        self.send_header('Content-type', 'text/html')
        self.end_headers()
        self.wfile.write(response.encode())

def run_server(port=8000):
    server_address = ('', port)
    httpd = HTTPServer(server_address, HelloWorldHandler)
    print(f'Server running on http://localhost:{port}')
    httpd.serve_forever()

if __name__ == '__main__':
    run_server()
