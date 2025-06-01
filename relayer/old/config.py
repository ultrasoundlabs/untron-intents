import json
import logging.config
import os
from decimal import Decimal
from typing import Dict, Any, List, Optional, TypedDict
from pathlib import Path

# Get the project root directory
PROJECT_ROOT = Path(__file__).parent.parent.absolute()

# Default paths
CONFIG_FILE = os.getenv("CONFIG_FILE", str(PROJECT_ROOT / "config.json"))
BACKUP_DIR = PROJECT_ROOT / "backups"
LOG_DIR = PROJECT_ROOT / "logs"

# Ensure directories exist
BACKUP_DIR.mkdir(exist_ok=True)
LOG_DIR.mkdir(exist_ok=True)

class TokenConfig(TypedDict):
    address: str
    static_fee: str
    percentage_fee_bps: int

class ChainConfig(TypedDict):
    name: str
    rpc: str
    transfers_contract_address: str
    receiver_factory_address: str
    tokens: Dict[str, TokenConfig]

class Config(TypedDict):
    ethereum_private_key: str
    tron_private_key: str
    trongrid_api_key: str
    chains: List[ChainConfig]

def get_env_override(key: str, default: Optional[str] = None) -> Optional[str]:
    """Get configuration override from environment variable."""
    env_key = f"UNTRON_{key.upper()}"
    return os.getenv(env_key, default)

def validate_hex_key(key: str, value: str) -> None:
    """Validate a hexadecimal private key."""
    if not value.startswith('0x'):
        raise ValueError(f"{key} must start with '0x'")
    if len(value) != 66:  # 0x + 64 hex chars
        raise ValueError(f"{key} must be 32 bytes (64 hex characters)")
    try:
        int(value[2:], 16)
    except ValueError:
        raise ValueError(f"{key} must be a valid hexadecimal string")

def validate_address(key: str, value: str) -> None:
    """Validate an Ethereum address."""
    if not value.startswith('0x'):
        raise ValueError(f"{key} must start with '0x'")
    if len(value) != 42:  # 0x + 40 hex chars
        raise ValueError(f"{key} must be 20 bytes (40 hex characters)")
    try:
        int(value[2:], 16)
    except ValueError:
        raise ValueError(f"{key} must be a valid hexadecimal string")

def load_config() -> Config:
    """Load and validate configuration from JSON file with environment overrides."""
    try:
        with open(CONFIG_FILE) as f:
            config = json.load(f)

        # Apply environment overrides
        for key in ['ethereum_private_key', 'tron_private_key', 'trongrid_api_key']:
            env_value = get_env_override(key)
            if env_value:
                config[key] = env_value

        # Validate private keys
        validate_hex_key('ethereum_private_key', config['ethereum_private_key'])
        validate_hex_key('tron_private_key', config['tron_private_key'])

        # Validate API key
        if not config['trongrid_api_key']:
            raise ValueError('trongrid_api_key cannot be empty')

        # Validate chain configurations
        if not config.get('chains'):
            raise ValueError('At least one chain configuration is required')

        for chain in config['chains']:
            # Check required fields
            for field in ['name', 'rpc', 'transfers_contract_address', 'receiver_factory_address', 'tokens']:
                if not chain.get(field):
                    raise ValueError(f"Missing required field '{field}' in chain configuration")

            # Validate contract addresses
            validate_address(f"transfers_contract_address for chain {chain['name']}", chain['transfers_contract_address'])
            validate_address(f"receiver_factory_address for chain {chain['name']}", chain['receiver_factory_address'])

            # Validate RPC URL
            if not chain['rpc'].startswith(('http://', 'https://', 'ws://', 'wss://')):
                raise ValueError(f"Invalid RPC URL format for chain {chain['name']}")

            # Validate tokens
            if not chain['tokens']:
                raise ValueError(f"At least one token configuration is required for chain {chain['name']}")

            for token_symbol, token_config in chain['tokens'].items():
                # Check required fields
                for field in ['address', 'static_fee', 'percentage_fee_bps']:
                    if field not in token_config:
                        raise ValueError(f"Missing required field '{field}' in token {token_symbol} configuration for chain {chain['name']}")

                # Validate token address
                validate_address(f"token address for {token_symbol} on chain {chain['name']}", token_config['address'])

                # Validate fees
                try:
                    static_fee = Decimal(token_config['static_fee'])
                    if static_fee < 0:
                        raise ValueError
                except (ValueError, TypeError):
                    raise ValueError(f"Invalid static_fee for token {token_symbol} on chain {chain['name']}. Must be a non-negative decimal.")

                try:
                    bps = int(token_config['percentage_fee_bps'])
                    if not 0 <= bps <= 10000:
                        raise ValueError
                except (ValueError, TypeError):
                    raise ValueError(f"Invalid percentage_fee_bps for token {token_symbol} on chain {chain['name']}. Must be an integer between 0 and 10000.")

        return config
        
    except FileNotFoundError:
        raise FileNotFoundError(f"Config file not found: {CONFIG_FILE}")
    except json.JSONDecodeError:
        raise ValueError(f"Invalid JSON in config file: {CONFIG_FILE}")

def load_abis() -> Dict[str, Any]:
    """Load contract ABIs from JSON files."""
    try:
        # Load factory and receiver ABIs
        factory_abi = json.load(open(PROJECT_ROOT / "out/ReceiverFactory.json"))["abi"]
        receiver_abi = json.load(open(PROJECT_ROOT / "out/UntronReceiver.json"))["abi"]
        transfers_abi = json.load(open(PROJECT_ROOT / "out/UntronTransfers.json"))["abi"]
        
        # Minimal ERC20 Transfer event ABI
        erc20_abi = [
            {
                "anonymous": False,
                "inputs": [
                    {"indexed": True, "name": "from", "type": "address"},
                    {"indexed": True, "name": "to", "type": "address"},
                    {"indexed": False, "name": "value", "type": "uint256"}
                ],
                "name": "Transfer",
                "type": "event"
            }
        ]
        
        return {
            "factory": factory_abi,
            "receiver": receiver_abi,
            "transfers": transfers_abi,
            "erc20": erc20_abi
        }
    except FileNotFoundError as e:
        raise FileNotFoundError(f"Missing ABI file: {e.filename}")

def setup_logging() -> None:
    """Configure application logging."""
    os.makedirs(LOG_DIR, exist_ok=True)
    
    logging_config = {
        "version": 1,
        "formatters": {
            "default": {
                "format": "%(asctime)s - %(name)s - %(levelname)s - %(message)s"
            },
        },
        "handlers": {
            "file": {
                "class": "logging.handlers.RotatingFileHandler",
                "filename": str(LOG_DIR / "relayer.log"),
                "maxBytes": 10*1024*1024,  # 10MB
                "backupCount": 5,
                "formatter": "default",
            },
            "console": {
                "class": "logging.StreamHandler",
                "formatter": "default"
            }
        },
        "root": {
            "handlers": ["file", "console"],
            "level": "INFO",
        },
    }
    
    logging.config.dictConfig(logging_config)

# Initialize configuration
CONFIG = load_config()
ABIS = load_abis()
setup_logging()

# Create logger for this module
logger = logging.getLogger(__name__)

# Constants for blockchain operations
ETHEREUM_POLL_INTERVAL = 2
TRON_POLL_INTERVAL = 2
MAX_RETRIES = 3
RETRY_DELAY = 1
GAS_PRICE_MULTIPLIER = 11 / 10  # 110% of base gas price
MAX_GAS_PRICE = 100_000_000_000  # 100 gwei

# Database settings
DATABASE_URL = os.getenv("DATABASE_URL", "sqlite:///receivers.db")
DB_FILENAME = str(PROJECT_ROOT / "receivers.db")

# Tron API settings
TRON_API_KEY = CONFIG["trongrid_api_key"]

# Cache settings
CACHE_EXPIRY = 3600  # 1 hour in seconds
MAX_CACHE_SIZE = 10000  # Maximum number of items in cache
