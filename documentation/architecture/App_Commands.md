# Application Commands (Business Logic)

This file defines the core commands that drive the WeaveLang Engine. These commands are available to both the GUI and the Terminal interface.

## Navigation & Selection
*   `select sentence <ID>` or `select sentence <Index>`
    *   *Effect:* Sets the active sentence context.
    *   *Example:* `select sentence S1`
*   `select range <StartID> <EndID>`
    *   *Effect:* Selects a range of sentences.
    *   *Example:* `select range S1 S10`
*   `set view <TierView>`
    *   *Effect:* Changes the active tier being viewed/edited (e.g., `base`, `advanced_target`, `basic_base`).
    *   *Example:* `set view advanced_target`

## Editing
*   `update text <SentenceID> <TierID> "<NewText>"`
    *   *Effect:* Updates the text content of a specific tier.
    *   *Example:* `update text S1 basic_base "The simplified text."`
*   `approve edits <SentenceID> <TierID>`
    *   *Effect:* Marks a Dirty tier as Valid/Clean.
    *   *Example:* `approve edits S1 basic_base`

## Generation (LLM)
*   `generate tier <SentenceID> <TierID>`
    *   *Effect:* Triggers an LLM job to generate the specified tier from its parent.
    *   *Example:* `generate tier S1 basic_target`
*   `generate mapping <SentenceID> <SourceTier> <TargetTier>`
    *   *Effect:* Triggers an LLM job to generate a mapping between two tiers.
    *   *Example:* `generate mapping S1 basic_base basic_target`
*   `apply collateral <Bool>`
    *   *Effect:* Accepts or rejects pending collateral updates.
    *   *Example:* `apply collateral true`

## Project Management
*   `load project <Path>`
*   `save project`
*   `set config <Key> <Value>`
    *   *Example:* `set config batch_size 10`
