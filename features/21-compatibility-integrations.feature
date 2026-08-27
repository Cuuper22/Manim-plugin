@F21 @compatibility
Feature: Perform explicit version and integration work

  Scenario: Migrate Manim Community source with evidence
    Given a project uses APIs removed by its selected Manim Community version
    When Codex performs the documented migration workflow
    Then each incompatible construct receives a documented replacement
    And a representative target-version preview is produced before the change is propagated

  Scenario: Keep Manim flavors separate
    Given a project declares ManimGL as its source flavor
    When it is migrated to Manim Community
    Then the output imports only Manim Community APIs
    And no ManimGL symbol remains unresolved

  Scenario: Record an explicitly selected extension
    Given the user has selected and verified an installed Manim extension
    When Codex authors source that depends on it
    Then its version is pinned in project setup metadata
    And namespaced project metadata records the dependency
    And Director does not claim compatibility beyond the rendered evidence
