Feature: Loading a configuration

  Several configurations are read together and merged into the one desired state a machine is
  converged against. A configuration the tool cannot honour is refused before anything is read
  from the machine.

  Scenario: A configuration written for the superseded format is refused by version
    Given Alice has a configuration declaring format version "0.1.0"
    When Alice loads her configurations
    Then loading is refused
    And the refusal mentions "0.1.0"
    And the refusal mentions "2"

  Scenario: Two configurations claiming the same link differently are refused
    Given Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration linking ".gitconfig" to "elsewhere/.gitconfig"
    When Alice loads her configurations
    Then loading is refused
    And the refusal mentions "conflicting claims"

  Scenario: A run reports every configuration it could not read
    Given Alice has a configuration declaring format version "0.1.0"
    And Alice has a configuration declaring format version "0.2.0"
    When Alice loads her configurations
    Then loading is refused
    And the refusal mentions "0.1.0"
    And the refusal mentions "0.2.0"

  Scenario: A configuration Alice can read is refused alongside one she cannot
    Given Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration declaring format version "0.1.0"
    When Alice loads her configurations
    Then loading is refused
    And no desired state is loaded

  Scenario: A refusal names the setting at fault, not only the configuration
    Given Alice has a configuration whose machine settings omit the repositories directory path
    When Alice loads her configurations
    Then loading is refused
    And the refusal mentions "machine"
    And the refusal mentions "repositories_directory_path"

  Scenario: Two configurations claiming the same link identically are one resource
    Given Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration linking ".gitconfig" to "gitconfig/.gitconfig"
    When Alice loads her configurations
    Then the desired state holds 1 symlink
