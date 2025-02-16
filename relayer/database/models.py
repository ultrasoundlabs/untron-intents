from sqlalchemy import Column, String, Integer, DateTime, Boolean
from sqlalchemy.ext.declarative import declarative_base
from sqlalchemy.sql import func

Base = declarative_base()

class Receiver(Base):
    """Model for tracking deployed receiver contracts."""
    __tablename__ = 'receivers'

    id = Column(Integer, primary_key=True)
    tron_address = Column(String, unique=True, nullable=False)
    eth_address = Column(String, unique=True, nullable=False)
    resolved_at = Column(DateTime, server_default=func.now())

class CaseFix(Base):
    """Model for tracking case-sensitive address mappings."""
    __tablename__ = 'case_fixes'

    id = Column(Integer, primary_key=True)
    lowercase = Column(String, unique=True, nullable=False)  # Lowercased version of the address
    original = Column(String, nullable=False)  # Original case-sensitive address
    created_at = Column(DateTime, server_default=func.now())

class ProcessedIntent(Base):
    """Model for tracking processed transfer intents to prevent duplicates.
    
    The eth_tx_hash serves as the primary key.
    """
    __tablename__ = 'processed_intents'

    eth_tx_hash = Column(String, primary_key=True)  # Hash of the Ethereum transaction that triggered this intent
    tron_tx_hash = Column(String)  # Hash of the Tron transaction that fulfilled this intent
    amount = Column(String, nullable=False)  # Amount as string to avoid precision loss
    token = Column(String, nullable=False)  # Token contract address or symbol
    source = Column(String, nullable=False)  # Either "receiver" or "order"
    is_claimed = Column(Boolean, default=False)  # Whether claim() was called on Ethereum
    created_at = Column(DateTime, server_default=func.now())
    updated_at = Column(DateTime, onupdate=func.now())
