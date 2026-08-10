Feature: Loading a configuration

  Several configurations are read together and merged into the one desired state a machine is
  converged against. Each configuration declares which machines it is for, and an invocation names
  which class of machine it is running on, so where configurations are read from cannot change which
  of them apply. A run reads one configuration for every machine and exactly one for this class, and
  a configuration the tool cannot honour is refused before anything is read from the machine.

  Scenario: A configuration for another class of machine is left out
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    And Alice's employer's repository has a configuration for work machines linking ".yarnrc" to "yarn/.yarnrc"
    When Alice loads her configurations for a personal machine
    Then the desired state links ".npmrc"
    And the desired state does not link ".yarnrc"

  Scenario: A configuration for every machine applies alongside the machine's own
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then the desired state links ".gitconfig"
    And the desired state links ".npmrc"

  Scenario: A machine nothing applies to is refused rather than converged against nothing
    Given Alice has a configuration for work machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "personal"

  Scenario: A set holding nothing for this machine's class is refused
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "machine's class"

  Scenario: A set holding nothing for every machine is refused
    Given Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "every machine"

  Scenario: A source whose configurations belong in two trees is refused
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for work machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "two trees"

  Scenario: A file that is not a configuration is left where it lies
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    And Alice keeps a "README.md" alongside her configurations
    When Alice loads her configurations for a personal machine
    Then the desired state links ".gitconfig"

  Scenario: A configuration for another machine is refused when it cannot be read
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for work machines declaring version "0.1.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "0.1.0"

  Scenario: A configuration whose version is not a generation is refused by version
    Given Alice has a configuration declaring version "0.1.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "0.1.0"
    And the refusal mentions the generation this build is

  Scenario: A configuration stating the generation below this build is read
    Given Alice has a configuration for every machine declaring the generation below this build linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then the desired state links ".gitconfig"

  Scenario: A configuration a generation back is rewritten rather than refused
    Given Alice has a configuration for every machine declaring the generation below this build linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then 1 configuration is waiting to be rewritten

  Scenario: A configuration already at this generation is rewritten by nothing
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".npmrc" to "npm/.npmrc"
    When Alice loads her configurations for a personal machine
    Then 0 configurations are waiting to be rewritten

  Scenario: A configuration further back than one generation is refused as outgrown
    Given Alice has a configuration declaring a generation this build has outgrown
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "intervening build"

  Scenario: A configuration needing a newer build names the generation it needs
    Given Alice has a configuration declaring a generation beyond this build
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions the generation the configuration needs
    And the refusal mentions the generation this build is

  Scenario: Two configurations claiming the same link differently are refused
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".gitconfig" to "elsewhere/.gitconfig"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "conflicting claims"

  Scenario: A run reports every configuration it could not read
    Given Alice has a configuration declaring version "0.1.0"
    And Alice has a configuration declaring version "0.2.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "0.1.0"
    And the refusal mentions "0.2.0"

  Scenario: A configuration Alice can read is refused alongside one she cannot
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration declaring version "0.1.0"
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And no desired state is loaded

  Scenario: A refusal names the setting at fault, not only the configuration
    Given Alice has a configuration naming no account to act as
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "github_account"

  Scenario: A configuration declaring no machines it is for is refused
    Given Alice has a configuration that declares no machines it is for
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "applies_to"

  Scenario: Two configurations claiming the same link identically are one resource
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice has a configuration for personal machines linking ".gitconfig" to "gitconfig/.gitconfig"
    When Alice loads her configurations for a personal machine
    Then the desired state holds 1 symlink

  Scenario: A directory inside no checkout has nothing to read a configuration's files out of
    Given Alice has a configuration for every machine linking ".gitconfig" to "gitconfig/.gitconfig"
    And Alice keeps her configurations outside any checkout
    When Alice loads her configurations for a personal machine
    Then loading is refused
    And the refusal mentions "no checkout"
