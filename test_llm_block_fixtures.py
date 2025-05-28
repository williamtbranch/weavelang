# test_llm_block_fixtures.py
# Contains string fixtures for testing validate_llm_block

# --- I. Good / Valid Blocks ---

GOOD_BLOCK_MINIMAL_CORRECT = """
AdvS:: Oración avanzada.
SimS:: Oración simple.
SimE:: Simple sentence.
SimS_Segments::
S1(Oración simple.)
PHRASE_ALIGN::
S1 ~ Oración avanzada. ~ Simple sentence.
SimSL::
S1 :: oración simple
AdvSL:: oración avanzado
DIGLOT_MAP::
S1 :: Simple->simple(simple)(Y) | sentence->oración(oración)(Y)
"""

GOOD_BLOCK_MULTI_SEGMENT_WITH_LOCKED = """
AdvS:: El gato rápido y marrón saltó sobre el perro perezoso.
SimS:: El gato rápido saltó. El gato marrón también saltó. El perro era flojo.
SimE:: The quick cat jumped. The brown cat also jumped. The dog was lazy.
SimS_Segments::
S1(El gato rápido saltó.)
S2(El gato marrón también saltó.)
S3(El perro era flojo.)
PHRASE_ALIGN::
S1 ~ El gato rápido ~ The quick cat jumped.
S2 ~ y marrón saltó ~ The brown cat also jumped.
S3 ~ sobre el perro perezoso. ~ The dog was lazy.
SimSL::
S1 :: el gato rápido saltar
S2 :: el gato marrón también saltar
S3 :: el perro ser flojo
AdvSL:: el gato rápido y marrón saltar sobre el perro perezoso
DIGLOT_MAP::
S1 :: The->el(El)(Y) | quick->rápido(rápido)(Y) | cat->gato(gato)(Y) | jumped->saltar(saltó)(Y)
S2 :: The->el(El)(Y) | brown->marrón(marrón)(Y) | cat->gato(gato)(Y) | also->también(también)(Y) | jumped->saltar(saltó)(Y)
S3 :: The->el(El)(Y) | dog->perro(perro)(Y) | was->ser(era)(Y) | lazy->flojo(flojo)(Y)
LOCKED_PHRASE:: S1 S2
"""

GOOD_BLOCK_EMPTY_SECTIONS_VALID = """
AdvS:: Ok.
SimS:: Ok.
SimE:: Ok.
SimS_Segments::
S1(Ok.)
PHRASE_ALIGN::
S1 ~ Ok. ~ Ok.
SimSL::
S1 :: ok
AdvSL:: 
DIGLOT_MAP::
S1 :: 
""" # Empty AdvSL and DIGLOT_MAP S1 content is valid

# --- II. Bad Blocks - Fundamental Structure ---

BAD_BLOCK_EMPTY = ""

BAD_BLOCK_WHITESPACE_ONLY = "   \n\n   \t  "

BAD_BLOCK_PREMATURE_END_SENTENCE = """
AdvS:: AdvS content.
END_SENTENCE
SimS:: SimS content.
"""

BAD_BLOCK_MISSING_REQUIRED_ADVS = """
SimS:: SimS content.
SimE:: SimE content.
SimS_Segments::
S1(segment)
PHRASE_ALIGN::
S1 ~ a ~ b
SimSL::
S1 :: l
AdvSL:: la
DIGLOT_MAP::
S1 :: E->S(F)(Y)
"""

BAD_BLOCK_DUPLICATE_SECTION_SIMS = """
AdvS:: AdvS content.
SimS:: SimS content 1.
SimE:: SimE content.
SimS:: SimS content 2.
SimS_Segments::
S1(segment)
PHRASE_ALIGN::
S1 ~ a ~ b
SimSL::
S1 :: l
AdvSL:: la
DIGLOT_MAP::
S1 :: E->S(F)(Y)
"""

BAD_BLOCK_WRONG_ORDER_SIMS_BEFORE_ADVS = """
SimS:: SimS content.
AdvS:: AdvS content.
SimE:: SimE content.
SimS_Segments::
S1(segment)
PHRASE_ALIGN::
S1 ~ a ~ b
SimSL::
S1 :: l
AdvSL:: la
DIGLOT_MAP::
S1 :: E->S(F)(Y)
"""

# --- III. Bad Blocks - SimS_Segments ---

BAD_BLOCK_SIMS_SEGMENTS_EMPTY_CONTENT = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
PHRASE_ALIGN::
S1 ~ a ~ b
SimSL::
S1 :: l
AdvSL:: la
DIGLOT_MAP::
S1 :: E->S(F)(Y)
""" # SimS_Segments marker present, but no segment lines

BAD_BLOCK_SIMS_SEGMENTS_MALFORMED_LINE = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
S2 segment two) 
PHRASE_ALIGN::
S1 ~ a ~ b
S2 ~ c ~ d
SimSL::
S1 :: l1
S2 :: l2
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
S2 :: E2->S2(F2)(Y)
""" # S2 missing opening parenthesis

BAD_BLOCK_SIMS_SEGMENTS_EMPTY_PAREN_CONTENT = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1() 
PHRASE_ALIGN::
S1 ~ a ~ b
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
"""

BAD_BLOCK_SIMS_SEGMENTS_DUPLICATE_ID = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
S1(segment one again) 
PHRASE_ALIGN::
S1 ~ a ~ b 
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
"""

BAD_BLOCK_SIMS_SEGMENTS_NON_SEQUENTIAL_ID = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
S3(segment three) 
PHRASE_ALIGN::
S1 ~ a ~ b
S3 ~ c ~ d
SimSL::
S1 :: l1
S3 :: l3
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
S3 :: E3->S3(F3)(Y)
"""

# --- IV. Bad Blocks - PHRASE_ALIGN ---

BAD_BLOCK_PHRASE_ALIGN_EMPTY_CONTENT = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
"""

BAD_BLOCK_PHRASE_ALIGN_MALFORMED_LINE = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1  span2 
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
""" # Missing a tilde

BAD_BLOCK_PHRASE_ALIGN_UNKNOWN_ID = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S2 ~ span1 ~ span2 
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
"""

BAD_BLOCK_PHRASE_ALIGN_MISSING_SPAN = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~  ~ span2 
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
"""

BAD_BLOCK_PHRASE_ALIGN_LINE_COUNT_MISMATCH = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
S2(segment two)
PHRASE_ALIGN::
S1 ~ span1 ~ span2 
SimSL::
S1 :: l1
S2 :: l2
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
S2 :: E2->S2(F2)(Y)
""" # PHRASE_ALIGN only has one line for two segments

# --- V. Bad Blocks - SimSL ---
# (Similar structure to PHRASE_ALIGN bad blocks: empty, malformed, unknown ID, line count mismatch)

BAD_BLOCK_SIMSL_UNKNOWN_ID = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S2 :: lemmaX 
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
"""

# --- VI. Bad Blocks - AdvSL ---

BAD_BLOCK_ADVSL_MULTIPLE_LINES = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S1 :: l1
AdvSL:: lemmaA lemmaB
lemmaC lemmaD
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
""" # Assuming AdvSL content should be logically one line

# --- VII. Bad Blocks - DIGLOT_MAP ---

BAD_BLOCK_DIGLOT_MISSING_S_LINE_FOR_SEGMENT = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(seg1)
S2(seg2)
PHRASE_ALIGN::
S1 ~ a ~ b
S2 ~ c ~ d
SimSL::
S1 :: l1
S2 :: l2
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y) 
""" # Missing S2 line in DIGLOT_MAP

BAD_BLOCK_DIGLOT_MALFORMED_ENTRY_SYNTAX = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: Eng1->Spa1_Form1_Y 
""" # Missing parentheses, etc.

BAD_BLOCK_DIGLOT_EMPTY_ENGWORD = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: ->Spa1(Form1)(Y) 
"""

BAD_BLOCK_DIGLOT_INVALID_VIABILITY_FLAG = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: Eng1->Spa1(Form1)(X) 
"""

BAD_BLOCK_DIGLOT_EXTRA_PIPE = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: Eng1->Spa1(Form1)(Y) || Eng2->Spa2(Form2)(N)
"""

# --- VIII. Bad Blocks - LOCKED_PHRASE (if present) ---

BAD_BLOCK_LOCKED_PHRASE_UNKNOWN_ID = """
AdvS:: a
SimS:: s
SimE:: e
SimS_Segments::
S1(segment one)
PHRASE_ALIGN::
S1 ~ span1 ~ span2
SimSL::
S1 :: l1
AdvSL:: la
DIGLOT_MAP::
S1 :: E1->S1(F1)(Y)
LOCKED_PHRASE:: S1 S5 
""" # S5 not in SimS_Segments

# --- Add more test cases as needed ---