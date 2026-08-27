@F19 @workbench
Feature: Inspect and control a project through the workbench

  Scenario: Select a declared scene
    Given a project with declared scenes is open
    When the user selects a scene in the project explorer
    Then the workbench shows its name, time range, and source location
    And the code tab loads the complete revision-locked source file

  Scenario: Save source without truncation or lost updates
    Given the complete source file and its revision are loaded
    When the user saves a source edit
    Then the server applies the replacement only if the revision still matches
    And the result records an undo snapshot

  Scenario: Control a queued render
    Given a render job is queued
    When the user cancels it in the workbench
    Then job state changes to cancelled
    And retry remains available with the same parameters

  Scenario: Keep visual controls honest while connected
    Given the workbench is connected to a live project
    When the inspector displays inferred object properties
    Then visual transform controls are read-only
    And source-backed changes are directed to the code editor or Codex project_apply
