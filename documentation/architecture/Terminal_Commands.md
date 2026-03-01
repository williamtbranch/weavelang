# Terminal Interface Commands

These commands are specific to the Terminal/CLI front-end. They control how the data is displayed in the terminal window but do not affect the underlying document state.

## View Control
*   `list nav`
    *   *Effect:* Lists sentences in the navigator view.
*   `list nav <Index>`
    *   *Effect:* Sets the viewport to start at `<Index>` and lists sentences.
    *   *Example:* `list nav 24` (Starts listing from sentence 24)
*   `show detail`
    *   *Effect:* Prints the details (tiers, mappings) of the currently selected sentence.
*   `show mapping`
    *   *Effect:* Prints the mapping table for the current selection.
*   `set displaysize <Size>`
    *   *Effect:* Sets how much data is printed to the screen at once in terms of lines. `<Size>` specifies the maximum number of lines to display. This should persist across sessions via a settings file.
    *   *Example:* `set displaysize 10` (Sets the display to show 10 lines at a time)


## Interaction
*   `watch job`
    *   *Effect:* Enters a mode that continuously prints progress of a running background job.
*   `clear`
    *   *Effect:* Clears the terminal screen.
*   `history`
    *   *Effect:* Shows command history.
*   `help`
    *   *Effect:* Lists available commands.
