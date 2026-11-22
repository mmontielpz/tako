# State-of-the-Art (SOTA) Discovery Report: Quantum Reservoir Computing

- **Source Mode:** Live OpenAlex API (metadata + reconstructed abstracts)
- **Research Objective:** Identify foundational classical and quantum reservoir computing works, important methodological developments, contributions and limitations.
- **Discovery Seeds:** Classical Reservoir Computing, Echo State Networks, Liquid State Machines, Quantum Reservoir Computing, Quantum circuit architectures
- **Papers Selected:** 10
- **Evidence Disclaimer:** Extractions are strictly `abstract-based` or `metadata-only` (no full-text PDF parsing). Missing fields are marked `UNKNOWN` or `NOT_VERIFIED`. Citation counts and venues are not treated as proof of scientific superiority.

## 1. Related Research Topics Discovered
Computer science, Reservoir computing, Quantum, Quantum computer, Echo state network, State (computer science), Echo (communications protocol), Quantum circuit, Qubit, Artificial intelligence, Benchmark (surveying), Quantum algorithm

## 2. Candidate Seminal & Foundational Works
- **2007 — Special issue on echo state networks and liquid state machines** (Herbert Jaeger; Wolfgang Maass; José Carlos Príncipe) — https://doi.org/10.1016/j.neunet.2007.04.001 (`metadata-only`)
- **2021 — Opportunities in Quantum Reservoir Computing and Extreme Learning Machines** (Pere Mujal; Rodrigo Martínez‐Peña; Johannes Nokkala; Jorge García‐Beni; Gian Luca Giorgi; Miguel C. Soriano et al.) — https://doi.org/10.1002/qute.202100027 (`abstract-based`)

## 3. Chronological Evolution
### 2007 — Special issue on echo state networks and liquid state machines
- **Authors:** Herbert Jaeger; Wolfgang Maass; José Carlos Príncipe | **Venue:** Neural Networks | **Evidence:** `metadata-only`
- **Source:** https://doi.org/10.1016/j.neunet.2007.04.001
- **Contribution:** UNKNOWN (metadata-only; no abstract accessible)
- **Reported results:** UNKNOWN (metadata-only; no abstract accessible)
- **Limitations:** UNKNOWN (metadata-only; no abstract accessible)

### 2021 — Opportunities in Quantum Reservoir Computing and Extreme Learning Machines
- **Authors:** Pere Mujal; Rodrigo Martínez‐Peña; Johannes Nokkala; Jorge García‐Beni; Gian Luca Giorgi; Miguel C. Soriano et al. | **Venue:** Advanced Quantum Technologies | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.1002/qute.202100027
- **Contribution:** In this review article, recent proposals and first experiments displaying a broad range of possibilities are reviewed when quantum inputs, quantum physical substrates and quantum tasks are considered.
- **Reported results:** Quantum reservoir computing and quantum extreme learning machines are two emerging approaches that have demonstrated their potential both in classical and quantum machine learning tasks. They exploit the quantumness of physical systems combined with an easy training strategy, achieving an excellent performance.
- **Limitations:** NOT_VERIFIED (limitations not stated in abstract)

### 2022 — Hybrid quantum-classical reservoir computing of thermal convection flow
- **Authors:** Philipp Pfeffer; Florian Heyder; Jörg Schumacher | **Venue:** Physical Review Research | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.1103/physrevresearch.4.033176
- **Contribution:** NOT_VERIFIED (not explicitly stated in abstract)
- **Reported results:** A comparison of the quantum reservoir computing model with its classical counterpart shows that the same prediction and reconstruction capabilities of classical reservoirs with thousands of perceptrons can be obtained by a few strongly entangled qubits. We demonstrate that the mean squared error between model output and ground truth in the test phase of the quantum reservoir computing algorithm increases when the reservoir is decomposed into separable subsets of qubits.
- **Limitations:** NOT_VERIFIED (limitations not stated in abstract)

### 2023 — Classical and quantum reservoir computing : development and applications in machine learning
- **Authors:** Laia Domingo | **Venue:** UPM Digital Archive (Technical University of Madrid) | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.20868/upm.thesis.76893
- **Contribution:** Además, la tesis explora el algoritmo de reservoir computing cuántico, una variante de computación cuántica del método, que tiene el potencial de manejar conjuntos de datos de mayor dimensión y generar representaciones cuánticas de los datos que serían difíciles de simular eficientemente utilizando métodos clásicos. The objective of this thesis is to investigate the principles of reservoir computing and develop state-of-the-art variants of the algorithm capable of addressing diverse applications in machine learning.
- **Reported results:** The research demonstrates the algorithm’s robustness and adaptability across very different domains, including agricultural time series forecasting and the time propagation of quantum systems. The results show that reservoir computing, together with a clever data decomposition in trend, seasonal and residual components, outperforms traditional forecasting methods.
- **Limitations:** Additionally, the thesis explores quantum reservoir computing, a quantum computing variant of the method, which has the potential to handle higher-dimensional datasets and generate quantum representations of data that would be challenging to simulate efficiently using classical methods. Moreover, the impact of quantum noise on the algorithm performance is evaluated, revealing that the amplitude damping noise can actually be beneficial for the performance of quantum reservoir computing, while the depolarizing and phase damping noise should be prioritized for correction.

### 2023 — Exploring quantumness in quantum reservoir computing
- **Authors:** Niclas Götting; Frederik Lohof; Christopher Gies | **Venue:** Physical Review A | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.1103/physreva.108.052427
- **Contribution:** NOT_VERIFIED (not explicitly stated in abstract)
- **Reported results:** Here, we establish a link between quantum properties of a quantum reservoir, namely, entanglement and its occupied phase-space dimension, and its linear short-term memory performance. We find that a high degree of entanglement in the reservoir is a prerequisite for a more complex reservoir dynamics, which is key to unlocking the exponential phase space and higher short-term memory capacity.
- **Limitations:** NOT_VERIFIED (limitations not stated in abstract)

### 2023 — Hybrid quantum-classical reservoir computing for simulating chaotic systems
- **Authors:** Filip Wudarski; O`Connor, Daniel; Shaun Geaney; Ata Akbari Asanjan; Max Wilson; Elena Strbac et al. | **Venue:** arXiv (Cornell University) | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.48550/arxiv.2311.14105
- **Contribution:** This work presents a hybrid quantum reservoir-computing (HQRC) framework, which replaces the reservoir in RC with a quantum circuit.
- **Reported results:** The noiseless simulations of HQRC demonstrate valid prediction times comparable to state-of-the-art classical RC models for both the Lorenz63 and double-scroll chaotic paradigmatic systems and adhere to the attractor dynamics long after the forecasts have deviated from the ground truth.
- **Limitations:** The noiseless simulations of HQRC demonstrate valid prediction times comparable to state-of-the-art classical RC models for both the Lorenz63 and double-scroll chaotic paradigmatic systems and adhere to the attractor dynamics long after the forecasts have deviated from the ground truth.

### 2023 — Quantum reservoir computing implementation on coherently coupled quantum oscillators
- **Authors:** Julien Dudas; Baptiste Carles; Erwan Plouet; Frank Alice Mizrahi; Julie Grollier; Danijela Marković | **Venue:** npj Quantum Information | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.1038/s41534-023-00734-4
- **Contribution:** We propose an implementation for quantum reservoir that obtains a large number of densely connected neurons by using parametrically coupled quantum oscillators instead of physically coupled qubits.
- **Reported results:** We obtain state-of-the-art accuracy of 99% on benchmark tasks that otherwise require at least 24 classical oscillators to be solved. Our results give the coupling and dissipation requirements in the system and show how they affect the performance of the quantum reservoir.
- **Limitations:** However, current approaches with qubits suffer from limited connectivity.

### 2023 — Quantum reservoir computing in finite dimensions
- **Authors:** Rodrigo Martínez‐Peña; Juan‐Pablo Ortega | **Venue:** Physical review. E | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.1103/physreve.107.035306
- **Contribution:** This paper shows that alternative representations can provide better insight when dealing with design and assessment questions. More explicitly, system isomorphisms are established that unify the density matrix approach to QRC with the representation in the space of observables using Bloch vectors associated with Gell-Mann bases.
- **Reported results:** This paper shows that alternative representations can provide better insight when dealing with design and assessment questions. It is shown that these vector representations yield state-affine systems previously introduced in the classical reservoir computing literature and for which numerous theoretical results have been established.
- **Limitations:** NOT_VERIFIED (limitations not stated in abstract)

### 2023 — Reduced-order modeling of two-dimensional turbulent Rayleigh-Bénard flow by hybrid quantum-classical reservoir computing
- **Authors:** Philipp Pfeffer; Florian Heyder; Jörg Schumacher | **Venue:** Physical Review Research | **Evidence:** `abstract-based`
- **Source:** https://doi.org/10.1103/physrevresearch.5.043242
- **Contribution:** Two hybrid quantum-classical reservoir computing models are presented to reproduce the low-order statistical properties of a two-dimensional turbulent Rayleigh-B\'enard convection flow at a Rayleigh number $Ra={10}^{5}$ and Prandtl number $Pr=10$.
- **Reported results:** The second of the two quantum circuit architectures, denoted H2, enables a complete execution of the reservoir update inside the quantum circuit without the usage of external memory. We analyze different measures of the reconstruction error in dependence on the hyperparameters which are specific for the quantum cases or shared with the classical counterpart, such as the reservoir size and the leaking rate.
- **Limitations:** NOT_VERIFIED (limitations not stated in abstract)

### 2026 — Quantum and Classical Reservoir Computing with Asymmetric Real-Anchoring for Audio Deepfake Detection: A Cross-Corpus Generalization Study
- **Authors:** Soham Gangopadhyay; Inderpreet Singh; Prateek Pandya; Ashish Mani; Sumit Goswami | **Venue:** SSRN Electronic Journal | **Evidence:** `metadata-only`
- **Source:** https://doi.org/10.2139/ssrn.7361255
- **Contribution:** UNKNOWN (metadata-only; no abstract accessible)
- **Reported results:** UNKNOWN (metadata-only; no abstract accessible)
- **Limitations:** UNKNOWN (metadata-only; no abstract accessible)

## 4. Known Limitations of This SOTA Run
- Uses OpenAlex metadata and reconstructed abstracts only (`full-text` not parsed).
- Sentence-level cue extraction can miss nuances; verify key claims against primary papers.
- Seminal-work flags are heuristic candidates only, not peer-reviewed endorsements.
