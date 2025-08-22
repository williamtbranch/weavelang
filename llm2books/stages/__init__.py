from .assemble_tiers import AssembleTiers
from .generate_simple_target import GenerateSimpleTarget
from .finalize_simple_target import FinalizeSimpleTarget

# --- NEW PHRASE-BASED DIGLOT STAGES ---
from .generate_phrase_map import GeneratePhraseMap
from .apply_phrase_mappings import ApplyPhraseMappings

# --- DEPRECATED STAGES (to be removed from orchestrator) ---
# from .generate_diglot_map import GenerateDiglotMap
# from .finalize_diglot_map import FinalizeDiglotMap

# --- REMAINING STAGES ---
from .generate_inverse_diglot_map import GenerateInverseDiglotMap
from .finalize_mappings import FinalizeMappings
from .finalize_book import FinalizeBook