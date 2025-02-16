from flask import Flask, request
import logging
import json

app = Flask(__name__)

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)

@app.before_request
def log_request():
    logger.info(f"Received {request.method} request to {request.url}")
    logger.info(f"Headers: {dict(request.headers)}")
    logger.info(f"Body: {request.get_data(as_text=True)}")

@app.route('/', defaults={'path': ''}, methods=['GET', 'POST', 'PUT', 'DELETE', 'PATCH'])
@app.route('/<path:path>', methods=['GET', 'POST', 'PUT', 'DELETE', 'PATCH'])
def catch_all(path):
    return json.dumps({"data": "0x" + b"\x22TEVr7jCiRofduU2wtQsMWLBr1m132A3S5j\x06totron\x03eth\x00".hex()})

if __name__ == '__main__':
    app.run(host='0.0.0.0', port=80)
