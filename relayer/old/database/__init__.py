import logging
from contextlib import asynccontextmanager
from sqlalchemy.ext.asyncio import create_async_engine, AsyncSession
from sqlalchemy.ext.asyncio import async_sessionmaker

from ..config import DATABASE_URL
from .models import Base

logger = logging.getLogger(__name__)

# Create async engine
engine = create_async_engine(
    DATABASE_URL.replace('sqlite:///', 'sqlite+aiosqlite:///'),
    echo=False
)

# Create async session factory
async_session = async_sessionmaker(
    engine,
    class_=AsyncSession,
    expire_on_commit=False
)

async def setup_database():
    """Initialize the database and create tables if they don't exist."""
    try:
        async with engine.begin() as conn:
            await conn.run_sync(Base.metadata.create_all)
        logger.info("Database tables created successfully")
    except Exception as e:
        logger.error(f"Error setting up database: {e}")
        raise

@asynccontextmanager
async def get_session():
    """Get a database session."""
    session = async_session()
    try:
        yield session
    finally:
        await session.close()