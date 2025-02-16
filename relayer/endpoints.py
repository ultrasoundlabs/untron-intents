import logging
from aiohttp import web
from sqlalchemy import select
from base58 import b58decode_check

from .database import get_session
from .database.models import Receiver, CaseFix
from .blockchain.ethereum import ethereum
from .utils import run_case_fix_binary
from .config import CONFIG

logger = logging.getLogger(__name__)

routes = web.RouteTableDef()

@routes.post("/resolve")
async def resolve_handler(request: web.Request) -> web.Response:
    """
    Handle CCIP-Read resolution requests.
    Expects POST request with JSON body containing hex-encoded domain data.
    """
    logger.info("=== Starting new resolve request ===")
    try:
        data = await request.json()
        logger.info(f"Received request data: {data}")
        
        domain = data.get("data")
        if not domain:
            logger.error("Missing data field in request")
            return web.json_response(
                {"message": "Missing data field"},
                status=400
            )
            
        try:
            domain = bytes.fromhex(domain.lstrip("0x"))
            logger.info(f"Successfully decoded domain bytes: {domain.hex()}")
        except Exception as e:
            logger.error(f"Failed to decode domain hex: {domain}, error: {e}")
            return web.json_response(
                {"message": f"Invalid domain format: {e}"},
                status=400
            )
            
        # Extract Tron address from domain data (DNS wire format)
        subdomain_length = domain[0]
        lowercased_tron_address = domain[1:subdomain_length+1].decode().lower()
        logger.info(f"Extracted Tron address from domain: {lowercased_tron_address}")
        
        # Check cache for case fix
        async with get_session() as session:
            case_fix = await session.execute(
                select(CaseFix).where(CaseFix.lowercase == lowercased_tron_address)
            )
            case_fix = case_fix.scalar_one_or_none()
            
            if case_fix:
                fixed_tron_address = case_fix.original
                logger.info(f"Found cached case fix: {lowercased_tron_address} -> {fixed_tron_address}")
            else:
                logger.info(f"No cached case fix found for {lowercased_tron_address}, running binary...")
                # Run case fix binary if not in cache
                fixed_tron_address = await run_case_fix_binary(lowercased_tron_address)
                if fixed_tron_address:
                    logger.info(f"Successfully fixed case: {lowercased_tron_address} -> {fixed_tron_address}")
                    session.add(CaseFix(
                        lowercase=lowercased_tron_address,
                        original=fixed_tron_address
                    ))
                    await session.commit()
                else:
                    logger.error(f"Failed to fix case for address: {lowercased_tron_address}")
                    return web.json_response(
                        {"message": "Failed to process Tron address"},
                        status=500
                    )
                
            # Decode Tron address
            try:
                raw_bytes = b58decode_check(fixed_tron_address)[1:]
            except Exception as e:
                logger.error(f"Failed to decode fixed Tron address: {fixed_tron_address} error: {e}")
                return web.json_response(
                    {"message": "Invalid Tron address"},
                    status=400
                )
            
            receiver_address = await ethereum.generate_receiver_address(raw_bytes)
            
            # Check if we have a receiver for this address
            receiver = await session.execute(
                select(Receiver).where(Receiver.eth_address == receiver_address)
            )
            receiver = receiver.scalar_one_or_none()
            
            if not receiver:
                # Store new receiver mapping
                session.add(Receiver(
                    eth_address=receiver_address,
                    tron_address=fixed_tron_address
                ))
                await session.commit()
                logger.info(f"Generated and stored new receiver: {receiver_address} -> {fixed_tron_address}")
            else:
                logger.info(f"Found existing receiver: {receiver_address}")
            
            # Construct response in DNS wire format
            result = "0x" + (bytes([subdomain_length]) + fixed_tron_address.encode() + domain[subdomain_length+1:]).hex()
            logger.info(f"=== Resolve complete: {lowercased_tron_address} -> {result} ===")
            
            return web.json_response({"data": result})
            
    except Exception as e:
        logger.exception(f"Unexpected error in resolve_handler: {e}")
        return web.json_response(
            {"message": f"Internal server error: {str(e)}"},
            status=500
        )

@routes.get("/health")
async def health_check(request):
    """Simple health check endpoint."""
    return web.json_response({"status": "ok"})

def setup_routes(app: web.Application) -> None:
    """Configure routes for the web application."""
    app.add_routes(routes)
