# Autobahn: Seamless high speed BFT - SOSP24 Artifact 
This is the repository for the Artifact Evaluation of SOSP'24 proceeding: "Autobahn: Seamless high speed BFT".

For all questions about the artifact please e-mail Neil Giridharan <giridhn@berkeley.edu> and Florian Suri-Payer <fsp@cs.cornell.edu>. 


# Table of Contents
1. [Artifact Overview](#artifact)
2. [High Level Claims](#Claims)
3. [Overview of steps to validate Claims](#validating)
4. [Installing Dependencies and Building Binaries](#installing)
5. [Setting up Cloud Environment](#cloud)
6. [Running Experiments](#experiments)


## Artifact Overview <a name="artifact"></a>

This artifact contains, and allows to reproduce, experiments for all figures included in the paper "Autobahn: Seamless high speed BFT". 

It contains a prototype implemententation of Autobahn, as well as the reference implementations used to evaluate baseline systems: VanillaHS, BatchedHS, and Bullshark. Each prototype is located on its *own* branch, named accordingly. For each system, we have provided *two* branches: one containing the base system (e.g. `autobahn`), and another that contains a version modified to simulate blips (e.g. `autobahn-blips`).
Please checkout the corresponding branch when validating claims for a given system and experiment.

Autobahn and all baseline systems are implemented in Rust, using the asynchronous Tokio runtime environment. TCP is used for networking, and ed25519-dalek signatures are used for authentication.
Replicas persist all messages receives to disk, using RocksDB.
Client processes connect to *local* Replica machines and submit dummy payload requests (transactions) only to this replica. Replicas distribute payloads to one another -- the specifics depend on the particular system. 

Orienting oneself in the code: 
For Autobahn and Bullshark, the two main code modules are `worker` and `primary`. The worker layer is responsible for receiving client requests. It forwards data and digests to the primary layer which contains the main consensus logic. The consensus logic is event driven: the core event loop is located in `primary/src/core.rs`, message reception is managed in `primary/src/primary.rs`. 
VanillaHS and BatchedHS consist of main modules `mempool` and `consensus`, which operate analogously: `mempool/src/core.rs` receives and forwards data, and `consensus/src/core.rs` contains the main consensus logic. 

## Concrete claims in the paper
Autobahn is a Byzantine Fault Tolerant (BFT) consensus protocol that aims to hit a sweet spot between high throughput, low latency, and the ability to recover from asynchrony (seamlessness).

- **Main claim 1**: Autobahn matches the Throughput of Bullshark, while reducing latency by a factor of ca. 2x. 

- **Main claim 2**: Autobahn avoids protocol-induced hangovers in the presence of blips. 



## Validating the Claims - Overview <a name="validating"></a>

All our experiments were run using Google Cloud Platform (GCP) (https://console.cloud.google.com/welcome/). To reproduce our results and validate our claims, you will need to 1) instantiate a matching GCP experiment, 2) build the prototype binaries, and 3) run the provided experiment scripts with the (supplied) configs we used to generate our results.

The ReadMe is organized into the following high level sections:

1. *Installing pre-requisites and building binaries*

   To build Autobahn and baseline source code in any of the branches several dependencies must be installed. Refer to section "Installing Dependencies" for detailed instructions on how to install dependencies and compile the code. 

2. *Setting up experiments on GCP* 

     To re-run our experiments, you will need to instantiate a distributed and replicated server (and client) configuration using GCP. 
     <!-- We have provided a public profile as well as public disk images that capture the configurations we used to produce our results. Section "Setting up Cloudlab" covers the necessary steps in detail. Alternatively, you may create a profile of your own and generate disk images from scratch (more work) - refer to section "Setting up Cloudlab" as well for more information. Note, that you will need to use the same Cluster (Utah) and machine types (m510) to reproduce our results. -->


3. *Running experiments*

     To reproduce our results you will need to checkout the respective branch, and run the supplied experiment scripts using the supplied experiment configurations. Section "Running Experiments" includes instructions for using the experiment scripts, modifying the configurations, and parsing the output. 
     

## Installing Dependencies <a name="installing"></a>

### Pre-requisites
We recommend running on Ubuntu 20.04 LTS as this is the environment we have used for our tests. This said, the code should compile and work on most operating systems.

We require several software dependencies. 
- python3
- rust (recommend 1.80 stable)
- clang version <= 14 (for building librocksdb, DO NOT use version 15 or higher)
- tmux

For convenience, we have provided an install script `install_deps.sh` in the `overview` branch that automatically installs the required dependencies.

After installation finishes, navigate to `autobahn-artifact/benchmark` and run `pip install -r requirements.txt`.

#### Manual installation
If not using `install_deps.sh` make sure to:
- update your distribution: `sudo apt-get update`
- use the script here: https://bootstrap.pypa.io/get-pip.py and not apt-get, and update the `PATH` environment variable to point to the location of pip.


### Building code: 
Finally, you can build the binaries (you will ned to do this anew on each branch):
Navigate to `autobahn-artifact` directory and build using `cargo build`.
Note: The experiment scripts will automatically build the binaries if they have not been yet. However, we recommend doing it separately to troubleshoot more easily.

## Testing Locally
To quickly confirm that the installation and build succeeded you may run a simple local experiment. 

In order to run a quick test locally:
1. checkout the branch `autobahn` (or checkout the appropriate branch for the system of your choice)
2. navigate to `autobahn-artifact/benchmark/`
3. run `fab local`.

This will run a simple local experiment, using the parameters provided in `fabfile.py` (in `def local()`). 
By default, the experiment is 20s long and uses 4 replicas. The output contains statistics for throughput, latency, etc.
Additional instructions can be found in `benchmark/README`.
> [!WARNING]
> The Readme in branches Autobahn and Bullshark also contains some instructions to run on AWS. 
> These are inherited from Narwhal/Bullshark and have NOT BEEN TESTED by us. 
> We recommend you use the GCP instructions that we have trialed ourselves.


## Setting up GCP
<!-- TODO: Can we provide alternatives on how to run elsehwere? (if the artifact committee cannot run on GCP) -->
<!-- Detail which machines and configs we used (CPU/SSD...). What geo setup (i.e. where machines are located) -->

> [!NOTE] 
> We strongly recommend running on GCP as our experiment scripts are designed to work with GCP. 
> New users to GCP can get $300 worth of free credit (https://console.cloud.google.com/welcome/new), which should be sufficient to reproduce our core claims. Unfortunately, trial access users *cannot* access the machine type used in our evaluation, and must instead use a weaker machine type (more details below). We have NOT evaluated or systems on these machine types. To accurately reproduce our results, we recommend using the same machine types employed in our experiments, and using the `SPOT`-market to save costs.

The Google Cloud console is the gateway for accessing all GCP services. You can search for services using the GCP console searchbar.

To create an account:
1. Select `Try For Free` (blue button)
2. Enter your account information for step 1 and step 2
3. Click `Start Free` (blue button)
4. Optionally complete the survey
5. Creating an account should automatically create a project called "My First Project". If not follow the instructions here to create a project: https://developers.google.com/workspace/guides/create-project
6. In the google cloud console search for compute engine API, and click the blue Enable button (this may take some time to complete). Do not worry about creating credentials for the API.

<!-- Most of the time we will use the compute engine service to create and manage VMs but occassionally we will use other services.  -->


### Setup SSH keys
In order to connect to GCP you will need to register an SSH key. 

Install ssh if you do not already have it (on Ubuntu this is `sudo apt-get install ssh`)

If you do not already have an ssh key-pair, run the following command locally to generate ssh keys: `ssh-keygen -t rsa -f ~/.ssh/KEY_FILENAME -C USERNAME -b 2048`

To add a public SSH key to the project metadata using the Google Cloud console, do the following:

1. In the Google Cloud console, go to the Metadata page.

2. Click the SSH keys tab.

3. Click Add SSH Key.

4. In the SSH key field that opens, add the public SSH key you generated earlier. 

> [!NOTE] 
> The key must be of the format: `KEY_VALUE USERNAME`.
> KEY_VALUE := the public SSH key value
> USERNAME := your username. For example, cloudysanfrancisco or cloudysanfrancisco_gmail_com. Note the USERNAME can't be root.

5. Click Save.

6. Remember the Username field (you will need it later for setting up the control machine)

### Setting up Google Virtual Private Cloud (VPC)
Next, you will need to create your own Virtual Private Cloud network. To do so: 

1. In the Google Cloud console, go to the VPC networks page.

2. Click Create VPC network.

3. Enter a Name for the network (we recommend `autobahn-vpc`). The description field is optional.

4. Maximum transmission unit (MTU): Choose 1460 (default)

5. Choose Automatic for the Subnet creation mode.

6. In the Firewall rules section, select the "autobahn-vpc-allow-internal", "autobahn-vpc-allow-ssh", "autobahn-vpc-allow-rdp", "autobahn-vpc-allow-icmp". The rules address common use cases for connectivity to instances.

7. Select Regional for the Dynamic routing mode for the VPC network.

8. Click Create. It may take some time for the vpc network to be created.

### Create Instance Templates
We're now ready to create an Instance Template for each region we need, containing the respective hardware configurations we will use.

We used the following four regsions in our experiments: 
- us-east5
- us-east1
- us-west1
- us-west4. 

Create one instance template per region:

1. In the Google Cloud console, go to the Instance templates page.

2. Click Create instance template.

3. Give instance template a name of your choice

4. Select the Location as follows: Choose Regional.

5. Select the Region where you want to create your instance template (one of us-east5, us-east1, us-west1, or us-west4).

6. Under Machine configuration select the T2D series (under General purpose category)

7. For Machine type select t2d-standard-16 (16vCPU, 64 GB of memory)
> [!NOTE] 
> Free users will only be able to create t2d-standard-4 instances.

8. Under Availability policies choose Spot for VM provisioning model (to save costs). The default spot VM settings are fine but make sure On VM termination is set to Stop.

9. Scroll down to Boot disk and click Change. For Operating system select Ubuntu. For Version make sure Ubunu 20.04 LTS is selected. For Boot disk type select Balanced persistent disk. This is important! If you use a HDD then writing to disk may become a bottleneck. For size put 20 GB. No need to change anything in advanced configuration.

10. Under Identity and API access select the "Allow full access to all Cloud APIs" option

11. The default Firewall options are fine (unchecked all boxes)

12. Under Network interfaces change the network interface to "autobahn-vpc". Subnetwork is Auto subnet. IP stack type is IPv4. External IPv4 address is Ephemeral. The Network Service Tier is Premium. Don't enable DNS PTR Record.

13. No need to add any additional disks

14. Under Security make sure Turn on vTPM and Turn on Integrity Monitoring is checked. Make sure "Block project-wide SSH keys" is unchecked

15. No need to change anything in the Management or Sole-tenancy sections

Finally, create one additional instance template to serve as the control machine. Make the following adjustments:
- Select `us-central1` as the region. 
- Name this instance template `autobahn-instance-template` (the scripts assume this is the name of the control machine).
- For this instance template select Standard instead of Spot for VM provisioning model (so it won't be pre-empted while running an experiment).
- We recommend you pick t2d-standard-4 (instead of t2d-standard-16) for the machine type for the control machine to save costs.

### Setting up Control Machine
We are now ready to set up our experiment controller. Follow these steps to instantiate the controller instance template:

1. In the Google cloud console go to the VM instances page

2. Select "Create instance" (blue button)

3. On the left sidebar select "New VM instance from template"


4. Select `autobahn-instance-template` from the list of templates

5. Change the name field to `autobahn-instance-template`

6. Double check that the rest of the options match the configurations in `autobahn-instance-template`

7. Wait for the instance to start (you can see the status indicator turn green when that is ready)

8. To connect to this instance from ssh copy the External IP address, and run the following command in the terminal:

`ssh -i SSH_PRIVATE_KEY_LOCATION USERNAME@EXTERNAL_IP_ADDRESS`, where SSH_PRIVATE_KEY_LOCATION is the path of the corresponding ssh private key, USERNAME is the username of the SSH key (found in the Metadata page under SSH keys), and EXTERNAL_IP_ADDRESS is the ip address of the control machine.

Next, we must setup the control machine environment:

1. Clone the repository. For convenience, we highly recommend you create two folders in the home directory on the control machine: `autobahn-bullshark` and `hotstuff-baselines`. Navigate to the `autobahn-bullshark` folder, clone the `autobahn-artifact` repo, and checkout branch `autobahn`. Then navigate to the `hotstuff-baselines` folder, clone the `autobahn-artifact` repo, and checkout the `vanilla-hs` branch. Having this structure will allow you to change parameters and run experiments for different baselines much faster than checking out different branches each time as Autobahn and Bullshark (and analogously VanillaHS and BatchedHS) share common parameter structure.

2. Install all required dependencies on the control machine. Follow the Install Dependencies section.

3. Generate new SSH keypairs for the control machine and add them to the metadata console. Follow the Generate SSH Keys section.

## Running Experiments

<!-- i.e. what scripts to run, what configs to give, and how to collect/interpret results.
-> fab remote -->

Now that you have setup GCP, you are ready to run experiments on GCP!
Follow the GCP Config instructions for both the `autobahn-bullshark` and `hotstuff-baselines` folders.

### GCP Config
The GCP config is found in `autobahn-artifact/benchmark/settings.json`. You will need to change the following:
1. `key`: change the `name` (name of the private SSH key) and `path` fields to match the key you generated in the prior section
Leave `port` unchanged (should be `5000`).

2. `repo`: Leave `name` unchanged (should be `autobahn-artifact`). You will need to change the `url` field to be the url of the artifact github repo. Specifically, you will need to prepend your personal access token to the beginning of the url. The url should be in this format: "https://TOKEN@github.com/neilgiri/autobahn-artifact", where `TOKEN` is the name of your personal access token. `branch` specifies which branch will be run on all the machines. This will determine which system ends up running. Only select an Autobahn or Bullshark branch if you are in the `autobahn-bullshark` folder. Similarly, only select a Vanilla HotStuff or Batched HotStuff branch if you are in the `hotstuff-baselines` folder.

3. `project_id`: the project id is found by clicking the the dropdown of your project (e.g. "My First Project") on the top left side, and looking at the ID field.

4. `instances`: `type` (value of t2d-standard-16) and `regions` (value of ["us-east1-b", "us-east5-a", "us-west1-b", "us-west4-a"]) should remain unchanged. If you select different regions then you will need to change the regions field to be the regions you are running in. You will need to change `templates` to be the names of the instance templates you created. The order matters, as they should correspond to the order of each region. The path should be in the format "projects/PROJECT_ID/regions/REGION_ID/instanceTemplates/TEMPLATE_ID", where PROJECT_ID is the id of the project you created in the prior section, REGION_ID is the name of the region without the subzone (i.e. us-east1 NOT us-east1-a).

### GCP Benchmark commands
1. If you want to run an Autobahn or Bullshark experiment navigate to `autobahn-bullshark/autobahn-artifact/benchmark`. If you want to run a Vanilla HotStuff or a Batched HotStuff experiment navigate to `hotstuff-baselines/autobahn-artifact/benchmark`.

2. For the first experiment, run `fab create` which will instantiate machines based off your instance templates. For subsequent experiments, you will not need to run `fab create` as the instances will already have been created. Anytime you delete the VM instances you will need to run `fab create` to recreate them. 
> [!NOTE] 
> Spot machines, although cheaper, are not reliable and may be terminated by GCP at any time. If this happens (perhaps an experiment fails), delete all other running instances and re-run `fab create`.

3. Then run `fab install` which will install rust and the dependencies on these machines. Like `fab create` you only need to run this command one time after the creation of the VMs.

4. Finally `fab remote` will launch a remote experiment with the parameters specified in `fabfile.py`. The next section will explain how to configure the parameters. The `fab remote` command should show a progress bar of how far along it is until completion. Note that the first time running the command may take a long time but subsequent trials should be faster.

## Configuring Parameters
The parameters for the remote experiment are found in `benchmark/fabfile.py`. To change the parameters locate the `remote(ctx, debug=True)` task section in `fabfile.py`. This task specifies two types of parameters, the benchmark parameters and the nodes parameters. 

The benchmark parameters look as follows:

```
bench_params = {
    'nodes': 4,        
    'workers': 1,
    'rate': 50_000,
    'tx_size': 512,
    'faults': 0,
    'duration': 20,
}
```


They specify the number of primaries (nodes) and workers per primary (workers) to deploy, the input rate (tx/s) at which the clients submits transactions to the system (rate), the size of each transaction in bytes (tx_size), the number of faulty nodes ('faults), and the duration of the benchmark in seconds (duration). 

The minimum transaction size is 9 bytes, this ensure that the transactions of a client are all different. 

The benchmarking script will deploy as many clients as workers and divide the input rate equally amongst each client. 
For instance, if you configure the testbed with 4 nodes, 1 worker per node, and an input rate of 1,000 tx/s (as in the example above), the scripts will deploy 4 clients each submitting transactions to one node at a rate of 250 tx/s. 

When the parameters faults is set to f > 0, the last f nodes and clients are not booted; the system will thus run with n-f nodes (and n-f clients).

The nodes parameters differ between each system. We show and example node parameters for Autobahn.

### Autobahn Parameters

```
node_params = {
    'header_size': 1_000,
    'max_header_delay': 100,
    'gc_depth': 50,
    'sync_retry_delay': 10_000,
    'sync_retry_nodes': 3,
    'batch_size': 500_000,
    'max_batch_delay': 100,
    'use_optimistic_tips': True,
    'use_parallel_proposals': True,
    'k': 4,
    'use_fast_path': True,
    'fast_path_timeout': 200,
    'use_ride_share': False,
    'car_timeout': 2000,
    
    'simulate_asynchrony': True,
    'asynchrony_type': [3],
    'asynchrony_start': [10_000], #ms
    'asynchrony_duration': [20_000], #ms
    'affected_nodes': [2],
    'egress_penalty': 50, #ms
    
    'use_fast_sync': True,
    'use_exponential_timeouts': False,
}
```
They are defined as follows.
> [!NOTE] 
> To reproduce our experiments you do NOT need to change any parameters. 

Protocol parameters:
- `header_size`: The preferred header size (= Car payload). Car proposals in Autobahn (and analogously DAG proposals in Bullshark) do not contain transactions themselves, but propose digests of mini-batches (see Eval section). The primary creates a new header when it has completed its previous Car (or for Bullshark, when it has enough DAG parents) and enough batches' digests to reach header_size. Denominated in bytes.
- `max_header_delay`: The maximum delay that the primary waits before readying a new header payload, even if the header did not reach max_header_size. Denominated in ms.
- `gc_depth`: The depth of the garbage collection (Denominated in number of rounds).
- `sync_retry_delay`: The delay after which the synchronizer retries to send sync requests in case there was no reply. Denominated in ms.
- `sync_retry_nodes`: Determine with how many nodes to sync when re-trying to send sync-request. These nodes are picked at random from the committee.
- `batch_size`: The preferred mini-batch size. The workers seal a batch of transactions when it reaches this size. Denominated in bytes.
- `max_batch_delay`: The delay after which the workers seal a batch of transactions, even if max_batch_size is not reached. Denominated in ms.

- `use_optimistic_tips`: Whether to enable Autobahn's optimistic tips optimization. If set to True then non-certified car proposals can be sent to consensus; if False, consensus proposals contain only certified car proposals.
- `use_parallel_proposals`: Whether to allow multiple active consensus instances at a time in parallel
- `k`: The maximum number of consensus instances allowed to be active at any time.
- `use_fast_path`: Whether to enable the 3f+1 fast path for consensus
- `fast_path_timeout`: The timeout for waiting for 3f+1 responses on the consensus fast path
- `use_ride_share`: DEPRECATED: Whether to enable the ride-sharing optimization of piggybacking consensus messages on car messages (see Autobahn supplemental material)
- `car_timeout`: The timeout for sending a car
- `use_fast_sync`: Whether to enable the fast sync optimization. If set to False, Autobahn will use the default recursive sync strategy utilized by DAG protocols
- `use_exponential_timeouts`: Whether to enable timeout doubling upon timeouts firing and triggering a View change

Blip simulation framework:
- `simulate_asynchrony`: Whether to simulate blips
- `asynchrony_type`: The specific type of blip.
- `asynchrony_start`: The start times for each blip event
- `asynchrony_duration`: The duration of each blip event
- `affected_nodes`: How many nodes experience blip behavior
- `egress_penalty`: DEPRECATED: For egress blips how much egress delay is added


The configs for each experimented are located the `experiment_configs` folder. To run a specific experiment copy and paste the experiment config into the fab remote task. For all experiments besides the scaling experiment you will want to make sure `nodes=1`. This will create 1 node per region specificed in the settings.json file.



## Reading Output Results
The experiment performance results are found in the `autobahn-artifact/benchmark/results` folder. 

Explain what file to look at for results, and which lines/numbers to look for.

Autobahn example: 200k tput. Consensus lat = from time it was proposed for consensus? End to end = from time it was received by replica?
 + RESULTS:
  Consensus TPS: 199,119 tx/s
 Consensus BPS: 101,948,918 B/s
 Consensus latency: 190 ms

 End-to-end TPS: 199,096 tx/s
 End-to-end BPS: 101,937,133 B/s
 End-to-end latency: 231 ms

 Blip graphs more difficult to read: Latency over time.

left number: time?, middle???   right number: latency in seconds? 

 5.748999834060669,6.148999929428101,0.40000009536743164
5.786999940872192,6.256999969482422,0.4700000286102295
5.786999940872192,6.194999933242798,0.40799999237060547
5.787999868392944,6.07699990272522,0.2890000343322754


## Reproducing Results
The exact configs and corresponding results for each of our eperiments can be found on branch `overview` in folder `autobahn-artifact/paper-results`.


Provide ALL configs for each experiment. But suggest they only validate the claims.
The experiment configs

### Performance under ideal conditions
When an experiment finishes the logs and output files are downloaded to the control machine. The performance results are found in `results/bench-

#### Autobahn
Peak throughput is around 234k txn/s, end-to-end latency is around 280 ms. The config to get the peak throughput is found in `autobahn-peak.txt`.

#### Bullshark
Peak throughput is around 234k txn/s, end-to-end latency is around 592 ms. 

#### Batched-HS
Peak throughput is around 189k txn/s, end-to-end latency is around 333 ms. 

#### Vanilla-HS
Peak throughput is around 15k txn/s, end-to-end latency is around 365 ms. 

- for each system, give our peak numbers + the config. Have them reproduce those

### Scalability
- for each n, and each system give the numbers (i.e. the whole fig as a table)

n=4 see main graph. We show here just n=20

#### Autobahn
Peak throughput is around 230 txn/s, end-to-end latency is around 303 ms. 

#### Bullshark
Peak throughput is around 230k txn/s, end-to-end latency is around 631 ms. 

#### Batched-HS
Peak throughput is around 110k txn/s, end-to-end latency is around 308 ms. 

#### Vanilla-HS
Peak throughput is around 1.5k txn/s, end-to-end latency is around 2002 ms. 

### Leader failures
- show the 3s blip in HS, and lack thereof for us (don't think we need to show the other two blips).
- give the config. Explain how to interpret the data file to see blip duration and hangover duration (Be careful to explain that the numbers can be slightly offset)

#### Autobahn
Blip duration: 1s (from 7 to 8?). Hangover: 0s

#### Vanilla-HS
Blip duration: 3s. Hangover: 4s


### Partition
- give the configs and run all. Same same.

#### Autobahn
Blip from 8 to 28. Hangover...  Measured lat.. (slightly higher than normal. Something with framework. Same for bullshark)

NOTE: GIVE OUR FIXED NUMBERS FROM REBUTTAL. SAY PAPER NUMBERS HAD A BUG..

#### Bullshark


#### Batched-HS


#### Vanilla-HS


