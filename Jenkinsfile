pipeline {
    agent any // Use a generic agent that has Docker installed
    environment {
        MAJOR_VERSION = '1'
        MINOR_VERSION = '0'
        PATCH_VERSION = "${env.BUILD_NUMBER}"
        DOCKER_IMAGE1 = 'evolvingsoftware/quantumforge_streaming'
        REGISTRY_URL = 'registry.evolvingsoftware.io'
        SEMANTIC_TAG = "${MAJOR_VERSION}.${MINOR_VERSION}.${PATCH_VERSION}"
        // Ensure Docker Buildx is available and set up properly
        DOCKER_BUILDKIT = 1
    }
    stages {
        stage('Checkout') {
            steps {
                checkout scm
            }
        }
        stage('Build and Push Docker Image 1') {
            steps {
                script {
                    // Using buildx to build and push the image
                    sh """
                    docker buildx build . -t ${REGISTRY_URL}/${DOCKER_IMAGE1}:${SEMANTIC_TAG} --push
                    """
                }
            }
        }
    }
}
