import logging
from aiohttp import web
from sqlalchemy import select
from base58 import b58decode_check
import asyncio

from .database import get_session
from .database.models import Receiver, CaseFix
from .blockchain.ethereum import ethereum
from .utils import run_case_fix_binary

logger = logging.getLogger(__name__)

# Global semaphore to limit concurrent binary executions
BINARY_SEMAPHORE = asyncio.Semaphore(1)  # Only allow one binary execution at a time

routes = web.RouteTableDef()

async def process_case_fix(lowercased_tron_address: str) -> str:
    """
    Process and cache the case fix result.
    Returns the fixed address if successful, empty string if failed.
    """
    try:
        async with get_session() as session:
            # Check if already processed
            case_fix = await session.execute(
                select(CaseFix).where(CaseFix.lowercase == lowercased_tron_address)
            )
            case_fix = case_fix.scalar_one_or_none()
            if case_fix:
                logger.info(f"Found cached case fix: {lowercased_tron_address} -> {case_fix.original}")
                return case_fix.original

            # Run case fix binary with semaphore protection
            logger.info(f"Waiting for binary semaphore for {lowercased_tron_address}")
            async with BINARY_SEMAPHORE:
                logger.info(f"Running case fix binary for {lowercased_tron_address}")
                fixed_tron_address = await run_case_fix_binary(lowercased_tron_address)
            
            if fixed_tron_address:
                logger.info(f"Successfully fixed case: {lowercased_tron_address} -> {fixed_tron_address}")
                session.add(CaseFix(
                    lowercase=lowercased_tron_address,
                    original=fixed_tron_address
                ))
                await session.commit()
                return fixed_tron_address
            else:
                logger.error(f"Failed to fix case for address: {lowercased_tron_address}")
                return ""
    except Exception as e:
        logger.exception(f"Error in case fix processing: {e}")
        return ""

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
        
        # Initialize app state if needed
        if not hasattr(request.app, 'background_tasks'):
            request.app['background_tasks'] = set()
        if not hasattr(request.app, 'in_progress_case_fixes'):
            request.app['in_progress_case_fixes'] = {}
            
        # Check if there's already a task running for this address
        existing_task = request.app['in_progress_case_fixes'].get(lowercased_tron_address)
        if existing_task and not existing_task.done():
            logger.info(f"Reusing existing case fix task for {lowercased_tron_address}")
            case_fix_task = existing_task
        else:
            # Create new task if none exists or previous one is done
            logger.info(f"Starting new case fix task for {lowercased_tron_address}")
            case_fix_task = asyncio.create_task(process_case_fix(lowercased_tron_address))
            request.app['background_tasks'].add(case_fix_task)
            request.app['in_progress_case_fixes'][lowercased_tron_address] = case_fix_task
            
            # Cleanup function to remove task from both tracking structures
            def cleanup_task(t):
                request.app['background_tasks'].discard(t)
                if request.app['in_progress_case_fixes'].get(lowercased_tron_address) == t:
                    request.app['in_progress_case_fixes'].pop(lowercased_tron_address, None)
            
            case_fix_task.add_done_callback(cleanup_task)
        
        try:
            # If client disconnects, task continues in background
            fixed_tron_address = await case_fix_task
            if not fixed_tron_address:
                return web.json_response(
                    {"message": "Failed to process Tron address"},
                    status=500
                )
        except asyncio.CancelledError:
            # Let the task continue in background if request is cancelled
            logger.info(f"Request cancelled, continuing case fix in background for: {lowercased_tron_address}")
            return web.json_response(
                {"message": "Request cancelled"},
                status=499  # Client Closed Request
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
        
        async with get_session() as session:
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
