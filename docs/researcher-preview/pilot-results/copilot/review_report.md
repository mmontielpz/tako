# SOTA Review Report: Quantum Reservoir Computing

- **Source Mode:** Live Provider-Native (copilot-cli)

## Challenged / Unsupported Claims
- The claim that the Echostate approach provides a verified fixed-reservoir/trainable-readout framework is weakened because the supplied DOI resolves to 'Deep Echo State Network (DeepESN): A Brief Survey,' not Jaeger's 2001 report.
- The statement that the 2017 disordered quantum-dynamics work established quantum reservoir computing is reasonable as foundational context, but the supplied evidence does not verify its implementation details, noise sensitivity, memory bounds, or benchmark results.
- The claim that spatial multiplexing increases nonlinear expressivity and memory is not established by the supplied bibliographic match alone; primary evidence is needed for subsystem independence, aggregate cost, scaling, and measured gains.
- The claim that noisy gate-based temporal processing provides a framework for practical quantum hardware is too strong because the supplied entry does not include verified analysis of decoherence, finite measurements, gate noise, or hardware scaling.
- The claim that Gaussian continuous-variable dynamics provide universal reservoir transformations is only supported at the title level by the verified bibliographic match; the supplied evidence does not substantiate universality conditions or temporal-task guarantees.
- Claims involving 'Quantum Reservoir Processing,' the 2021 extreme-learning review, and 'Higher-Order Quantum Reservoir Computing' are unsupported by the cited bibliographic records and should not be used as substantive evidence.
- The comparative statement that quantum reservoirs add higher-order observables and extreme-learning-style readouts lacks verified primary sources for those specific mechanisms in the supplied entries.

## Missing Foundational Works
- The original Echo State Networks foundational source, Jaeger's GMD Report 148, needs to be cited by a matching and verifiable primary identifier rather than the mismatched DOI.
- A classical reservoir-computing unification work is missing, particularly Jaeger and Haas, 'Harnessing Nonlinearity: Predicting Chaotic Systems and Saving Energy in Wireless Communication,' Science 304 (2004).
- A general reservoir-computing framework or survey is missing, such as Lukoševičius and Jaeger, 'Reservoir Computing Approaches to Recurrent Neural Network Training,' Computer Science Review 3 (2009), or Verstraeten et al.'s experimental comparison.
- The foundational extreme-learning-machine source is missing: Huang, Zhu, and Siew, 'Extreme learning machine: theory and applications,' Neurocomputing 70 (2006).
- The 2021 higher-order, extreme-learning, or review claims require an identified, DOI-matching primary article or review before they can be treated as foundational.

## Actionable Corrections & Uncertainty Labels
- For 'The Echo State Approach to Analysing and Training Recurrent Neural Networks,' remove the current DOI/ResearchGate URL and set Evidence level to NOT_VERIFIED until a primary GMD Report 148 identifier is confirmed.
- For 'Quantum Reservoir Processing,' replace the supplied DOI 10.1038/s41534-019-0174-1 with the verified OA DOI 10.1038/s41534-019-0149-8 and retain the article as bibliographically verified rather than mismatch-identified.
- For 'Opportunities in Quantum Reservoir Computing and Extreme Learning Machines,' remove the title-author claims associated with DOI 10.1002/qute.202100094 because that DOI resolves to 'Imaging Spin-Wave Damping Underneath Metals Using Electron Spins in Diamond.'
- For 'Higher-Order Quantum Reservoir Computing,' remove the article's title, author, venue, contribution, and results claims associated with DOI 10.1103/PhysRevLett.127.260401 because that DOI resolves to 'Learning Temporal Quantum Tomography.'
- Downgrade entries with NOT_VERIFIED or abstract-only evidence so that only bibliographic match and publication existence are stated; do not infer numerical findings, benchmarks, hardware advantages, or universal guarantees.
- For 'Gaussian States of Continuous-Variable Quantum Systems Provide Universal Quantum Reservoir Computing,' preserve the verified bibliographic match but change unsupported sections to NOT_VERIFIED except for the title-supported subject of Gaussian continuous-variable reservoir universality.
- For verified but only bibliographically matched entries, separate 'what the work is' from 'what it demonstrates': retain reservoir architecture and topic descriptions, but omit reported results and limitations until extracted from the primary text.

## Optional Deterministic Bibliographic Verification (OpenAlex)
- **The "Echo State" Approach to Analysing and Training Recurrent Neural Networks** (https://doi.org/10.13140/RG.2.1.4428.0645): `MISMATCH`
- **Real-Time Computing Without Stable States: A New Framework for Neural Computation Based on Perturbations** (https://doi.org/10.1162/089976602760407955): `VERIFIED`
- **Harnessing Disordered-Ensemble Quantum Dynamics for Machine Learning** (https://doi.org/10.1103/PhysRevApplied.8.024030): `VERIFIED`
- **Quantum Reservoir Processing** (https://doi.org/10.1038/s41534-019-0174-1): `MISMATCH`
- **Boosting Computational Power through Spatial Multiplexing in Quantum Reservoir Computing** (https://doi.org/10.1103/PhysRevApplied.11.034021): `VERIFIED`
- **Optimizing a Quantum Reservoir Computer for Time Series Prediction** (https://doi.org/10.1038/s41598-020-71673-9): `VERIFIED`
- **Temporal Information Processing on Noisy Quantum Computers** (https://doi.org/10.1103/PhysRevApplied.14.024065): `VERIFIED`
- **Opportunities in Quantum Reservoir Computing and Extreme Learning Machines** (https://doi.org/10.1002/qute.202100094): `MISMATCH`
- **Gaussian States of Continuous-Variable Quantum Systems Provide Universal Quantum Reservoir Computing** (https://doi.org/10.1038/s42005-021-00556-w): `VERIFIED`
- **Higher-Order Quantum Reservoir Computing** (https://doi.org/10.1103/PhysRevLett.127.260401): `MISMATCH`
