@F12 @assets
Feature: Manage visual and audio assets with provenance

  Scenario: Inventory the recurrence knot with provenance
    Given assets/manifest.json declares recurrence-knot.svg as required
    And the SVG exists inside the project asset directory
    When the asset manifest is refreshed
    Then the asset record contains its project-relative path and media type
    And its origin and license remain attached to the asset record

  Scenario: Handle a missing decorative asset
    Given an optional decorative asset does not exist
    When Codex authors the scene
    Then an approved fallback is used or the asset is omitted
    And no broken path reaches Manim

  Scenario: Block a missing required asset
    Given a required asset does not exist
    When the scene render attempts to load it
    Then the render is marked failed rather than complete
    And the diagnostic names the missing project-relative path
