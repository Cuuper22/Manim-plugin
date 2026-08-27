@F24 @reliability @security
Feature: Execute reproducibly and report success honestly

  Scenario: Require a real isolation boundary for untrusted code
    Given scene code is not trusted
    When a host render is considered
    Then Director identifies the scene as executable Python rather than passive media
    And the host render is not presented as sandboxed
    And the operator is directed to apply container or VM isolation before execution

  Scenario: Reject false render success
    Given Manim exits with code zero
    And no readable artifact of the requested type exists
    When job completion is evaluated
    Then the job is marked failed
    And the report says the output is missing

  Scenario: Retry deterministically
    Given a deterministic render was interrupted
    When the same render request is submitted again
    Then source, assets, configuration, seed, and command produce the same content fingerprint
    And only a complete validated cache entry may be reused
    And interrupted partial output is never reported as complete

  Scenario Outline: Enforce implemented worker budgets
    Given a worker exceeds its declared <budget>
    When the limit is reached
    Then the worker terminates the render safely
    And partial state is recorded as cancelled or failed rather than complete

    Examples:
      | budget                           |
      | wall-clock time limit            |
      | Unix address-space memory limit  |
