#!/usr/bin/env bash

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
NETWORK_NAME="conet-test-net"
CONTAINER1_NAME="moon"
CONTAINER2_NAME="sun"
IMAGE_NAME="conet:test"

# Function to print colored output
print_status() {
	echo -e "${GREEN}[INFO]${NC} $1"
}

print_warning() {
	echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
	echo -e "${RED}[ERROR]${NC} $1"
}

# Function to cleanup resources on exit
cleanup() {
	print_status "Cleaning up resources..."

	# Stop and remove containers
	if docker ps -q -f name="${CONTAINER1_NAME}" | grep -q .; then
		print_status "Stopping container ${CONTAINER1_NAME}..."
		docker stop "${CONTAINER1_NAME}" || true
	fi

	if docker ps -q -f name="${CONTAINER2_NAME}" | grep -q .; then
		print_status "Stopping container ${CONTAINER2_NAME}..."
		docker stop "${CONTAINER2_NAME}" || true
	fi

	docker rm -f "${CONTAINER1_NAME}" "${CONTAINER2_NAME}" 2>/dev/null || true

	# Remove network
	if docker network ls -q -f name="${NETWORK_NAME}" | grep -q .; then
		print_status "Removing network ${NETWORK_NAME}..."
		docker network rm "${NETWORK_NAME}" || true
	fi

	print_status "Cleanup completed"
}

# Set trap to cleanup on script exit
trap cleanup EXIT INT TERM

# Function to build Docker image
build_image() {
	print_status "Building Docker image..."
	if ! docker build -t "${IMAGE_NAME}" .; then
		print_error "Failed to build Docker image"
		exit 1
	fi
	print_status "Docker image built successfully"
}

# Function to create Docker network
create_network() {
	print_status "Creating Docker network: ${NETWORK_NAME}"
	if ! docker network create --subnet=172.18.0.0/16 "${NETWORK_NAME}"; then
		print_error "Failed to create Docker network"
		exit 1
	fi
	print_status "Docker network created successfully"
}

# Function to start containers
start_containers() {
	print_status "Starting containers..."

	# Start moon container
	docker run -d \
		--name "${CONTAINER1_NAME}" \
		--network "${NETWORK_NAME}" \
		--ip 172.18.0.2 \
		--privileged \
		-v "$(pwd)/tests/simple-connectivity/configs/moon.toml:/config/moon.toml:ro" \
		-v "/tmp/registry_updated.toml:/config/registry.toml:ro" \
		"${IMAGE_NAME}" \
		conet -c /config/moon.toml

	# Start sun container
	docker run -d \
		--name "${CONTAINER2_NAME}" \
		--network "${NETWORK_NAME}" \
		--ip 172.18.0.3 \
		--privileged \
		-v "$(pwd)/tests/simple-connectivity/configs/sun.toml:/config/sun.toml:ro" \
		-v "/tmp/registry_updated.toml:/config/registry.toml:ro" \
		"${IMAGE_NAME}" \
		conet -c /config/sun.toml

	print_status "Containers started successfully"
}

# Function to wait for containers to initialize
wait_for_startup() {
	print_status "Waiting for containers to initialize..."
	sleep 5

	# Check if containers are running
	if ! docker ps | grep -q "${CONTAINER1_NAME}"; then
		print_error "Container ${CONTAINER1_NAME} is not running"
		docker logs "${CONTAINER1_NAME}"
		exit 1
	fi

	if ! docker ps | grep -q "${CONTAINER2_NAME}"; then
		print_error "Container ${CONTAINER2_NAME} is not running"
		docker logs "${CONTAINER2_NAME}"
		exit 1
	fi

	print_status "Containers are running"
}

# Function to test connectivity
test_connectivity() {
	print_status "Testing connectivity between nodes..."

	# Test ping from sun to moon
	print_status "Pinging from sun to moon (10.10.10.1)..."
	if docker exec "${CONTAINER2_NAME}" ping -c 3 -W 5 10.10.10.1; then
		print_status "✓ Ping successful: Sun can reach Moon"
	else
		print_error "✗ Ping failed: Sun cannot reach Moon"
		return 1
	fi

	# Test ping from moon to sun
	print_status "Pinging from moon to sun (10.10.10.10)..."
	if docker exec "${CONTAINER1_NAME}" ping -c 3 -W 5 10.10.10.10; then
		print_status "✓ Ping successful: Moon can reach Sun"
	else
		print_error "✗ Ping failed: Moon cannot reach Sun"
		return 1
	fi

	print_status "✓ All connectivity tests passed!"
	return 0
}

# Function to show container logs for debugging
show_logs() {
	print_warning "Showing container logs for debugging..."

	echo "=== Moon container logs ==="
	docker logs "${CONTAINER1_NAME}" 2>&1 | tail -20

	echo -e "\n=== Sun container logs ==="
	docker logs "${CONTAINER2_NAME}" 2>&1 | tail -20
}

# Main test execution
main() {
	print_status "Starting conet connectivity test..."

	# Check if Docker is running
	if ! docker info >/dev/null 2>&1; then
		print_error "Docker is not running or not accessible"
		exit 1
	fi

	# Build image
	build_image

	# Create network
	create_network

	# Start containers
	start_containers

	# Wait for startup
	wait_for_startup

	# Test connectivity
	if test_connectivity; then
		print_status "✓ Test completed successfully!"
		exit 0
	else
		print_error "✗ Test failed!"
		show_logs
		exit 1
	fi
}

# Run main function
main "$@"
